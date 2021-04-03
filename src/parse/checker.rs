use rust_decimal::prelude::Zero;
use std::collections::{HashMap, HashSet};

use crate::{
    options::*,
    parse::{AccountInfoDraft, CostBasis, LedgerDraft, PostingDraft, TxnDraft},
    utils::parse_decimal,
    Account, AccountInfo, Amount, BalanceSheet, Currency, Date, Decimal, Error, ErrorLevel,
    ErrorType, Ledger, Posting, Price, Source, Transaction, TxnFlag, UnitCost,
};

impl UnitCost {
    fn matches(&self, unit_cost_amount: &Option<Amount>, date: &Option<Date>) -> bool {
        unit_cost_amount
            .as_ref()
            .map_or(true, |amount| amount.eq(&self.amount))
            && date.map_or(true, |date| date == self.date)
    }
}

macro_rules! filter_note_doc {
    ($items:ident, $open_date:ident, $valid_close:ident, $errors:ident) => {
        $items
            .into_iter()
            .filter(|item| {
                if item.date < $open_date {
                    $errors.push(Error {
                        level: ErrorLevel::Error,
                        r#type: ErrorType::Account,
                        src: item.src.clone(),
                        msg: "Reference to a not-yet-opened account.".to_string(),
                    });
                    false
                } else if let Some((close_date, _)) = &$valid_close {
                    if item.date > *close_date {
                        $errors.push(Error {
                            level: ErrorLevel::Error,
                            r#type: ErrorType::Account,
                            src: item.src.clone(),
                            msg: "Reference to a closed account.".to_string(),
                        });
                        false
                    } else {
                        true
                    }
                } else {
                    true
                }
            })
            .collect()
    };
}

fn check_accounts(
    accounts: HashMap<Account, AccountInfoDraft>,
    errors: &mut Vec<Error>,
) -> HashMap<Account, AccountInfo> {
    let mut result = HashMap::new();
    for (account, info_draft) in accounts {
        let AccountInfoDraft {
            open,
            close,
            currencies,
            notes,
            docs,
            meta,
        } = info_draft;
        if let Some((open_date, open_src)) = open {
            let valid_close = if let Some((close_date, close_src)) = close {
                if close_date < open_date {
                    errors.push(Error {
                        level: ErrorLevel::Error,
                        r#type: ErrorType::Account,
                        src: close_src,
                        msg: format!("{} closed before being opened.", &account),
                    });
                    None
                } else {
                    Some((close_date, close_src))
                }
            } else {
                None
            };
            let valid_notes = filter_note_doc!(notes, open_date, valid_close, errors);
            let valid_docs = filter_note_doc!(docs, open_date, valid_close, errors);
            let valid_info = AccountInfo {
                open: (open_date, open_src),
                close: valid_close,
                currencies,
                notes: valid_notes,
                docs: valid_docs,
                meta,
            };
            result.insert(account, valid_info);
        } else {
            let msg = format!("Reference to an unknown account {}.", &account);
            for note in notes {
                errors.push(Error {
                    level: ErrorLevel::Error,
                    r#type: ErrorType::Account,
                    src: note.src,
                    msg: msg.clone(),
                });
            }
            for doc in docs {
                errors.push(Error {
                    level: ErrorLevel::Error,
                    r#type: ErrorType::Account,
                    src: doc.src,
                    msg: msg.clone(),
                });
            }
            if let Some((_, close_src)) = close {
                errors.push(Error {
                    level: ErrorLevel::Error,
                    r#type: ErrorType::Account,
                    src: close_src,
                    msg: msg,
                });
            }
        }
    }
    result
}

fn check_posting(
    posting: &PostingDraft,
    txn_date: Date,
    accounts: &HashMap<Account, AccountInfo>,
) -> Result<(), String> {
    let account = &posting.account;
    if let Some(info) = accounts.get(account) {
        if txn_date < info.open.0 {
            return Err(format!("{} unopened as of {}.", account, txn_date));
        }
        if let Some((close_date, _)) = info.close {
            if txn_date > close_date {
                return Err(format!("{} closed as of {}.", account, txn_date));
            }
        }
        if let Some(Amount {
            number: _,
            currency,
        }) = &posting.amount
        {
            if info.currencies.len() > 0 && !info.currencies.contains(currency) {
                return Err(format!(
                    "{} not in the allowed currency set of {}: {:?}.",
                    currency, account, info.currencies
                ));
            }
        }
        Ok(())
    } else {
        Err(format!("Reference to unknown account {}.", account))
    }
}

fn is_opening_new(
    p_number: Decimal,
    running_balance: Option<&HashMap<Option<UnitCost>, Decimal>>,
) -> bool {
    if let Some(running_balance) = running_balance {
        for (cost, number) in running_balance {
            if cost.is_none() {
                continue;
            }
            if (number.is_sign_negative() && p_number.is_sign_positive())
                || (number.is_sign_positive() && p_number.is_sign_negative())
            {
                return false;
            } else {
                return true;
            }
        }
    }
    true
}

enum PostResult {
    Success(Posting),
    Expanded(Vec<Posting>),
    NeedInfer(PostingDraft),
    Fail,
    None,
}

fn close_position(
    posting: PostingDraft,
    running_balance: Option<&HashMap<Option<UnitCost>, Decimal>>,
    pending_change: &mut HashMap<Option<UnitCost>, Decimal>,
    per_currency_change: &mut HashMap<String, Decimal>,
    errors: &mut Vec<Error>,
) -> PostResult {
    let cost_literal = posting.cost.as_ref().unwrap();
    let p_amount = posting.amount.as_ref().unwrap();
    match (&cost_literal.basis, &cost_literal.date) {
        (None, None) => {
            if let Some(holding_balance) = running_balance {
                let total_holding: Decimal = holding_balance
                    .iter()
                    .map(|(cost, number)| {
                        if cost.is_some() {
                            *number
                        } else {
                            Decimal::zero()
                        }
                    })
                    .sum();
                if (total_holding + p_amount.number).is_zero() {
                    let PostingDraft {
                        account,
                        amount: _,
                        cost: _,
                        price: _,
                        meta,
                        src,
                    } = posting;
                    let mut expanded_postings = Vec::new();
                    for (unit_cost, holding_number) in holding_balance {
                        if let Some(unit_cost) = unit_cost {
                            *per_currency_change
                                .entry(unit_cost.amount.currency.to_string())
                                .or_default() -= unit_cost.amount.number * holding_number;
                            *pending_change.entry(Some(unit_cost.clone())).or_default() -=
                                holding_number;
                            let expanded_posting = Posting {
                                account: account.clone(),
                                amount: Amount {
                                    number: -holding_number,
                                    currency: p_amount.currency.clone(),
                                },
                                cost: Some(unit_cost.clone()),
                                price: None,
                                meta: meta.clone(),
                                src: src.clone(),
                            };
                            expanded_postings.push(expanded_posting);
                        }
                    }
                    PostResult::Expanded(expanded_postings)
                } else {
                    let error = Error {
                        r#type: ErrorType::NoMatch,
                        level: ErrorLevel::Error,
                        msg: format!("Account only has {} {}.", total_holding, p_amount.currency),
                        src: posting.src.clone(),
                    };
                    errors.push(error);
                    PostResult::Fail
                }
            } else {
                if !p_amount.number.is_zero() {
                    let error = Error {
                        r#type: ErrorType::NoMatch,
                        level: ErrorLevel::Error,
                        msg: format!("Account has no {}.", p_amount.currency),
                        src: posting.src.clone(),
                    };
                    errors.push(error);
                    PostResult::Fail
                } else {
                    PostResult::None
                }
            }
        }
        (Some(basis), Some(date)) => {
            let unit_cost_amount = basis.to_unit_cost(p_amount.number);
            let unit_cost_number = unit_cost_amount.number;
            let unit_cost = Some(UnitCost {
                amount: unit_cost_amount,
                date: *date,
            });
            let holding_number = running_balance
                .and_then(|m| m.get(&unit_cost))
                .copied()
                .unwrap_or_default();
            if holding_number.abs() < p_amount.number.abs() {
                let error = Error {
                    r#type: ErrorType::NoMatch,
                    level: ErrorLevel::Error,
                    msg: format!(
                        "Account only has {} {} {}.",
                        holding_number,
                        p_amount.currency,
                        &unit_cost.unwrap()
                    ),
                    src: posting.src.clone(),
                };
                errors.push(error);
                PostResult::Fail
            } else {
                *per_currency_change
                    .entry(basis.currency().to_string())
                    .or_default() += unit_cost_number * p_amount.number;
                *pending_change.entry(unit_cost.clone()).or_default() += p_amount.number;
                let PostingDraft {
                    account,
                    amount: _,
                    cost: _,
                    price,
                    meta,
                    src,
                } = posting;
                let valid_posting = Posting {
                    account,
                    amount: p_amount.clone(),
                    cost: unit_cost,
                    price,
                    meta,
                    src,
                };
                PostResult::Success(valid_posting)
            }
        }
        (Some(_), None) | (None, Some(_)) => {
            let unit_cost_amount = cost_literal
                .basis
                .as_ref()
                .map(|basis| basis.to_unit_cost(p_amount.number));
            let candidates = running_balance.map_or(Vec::new(), |m| {
                m.iter()
                    .filter(|(maybe_unit_cost, _)| {
                        maybe_unit_cost.as_ref().map_or(false, |unit_cost| {
                            unit_cost.matches(&unit_cost_amount, &cost_literal.date)
                        })
                    })
                    .collect()
            });
            match candidates.len() {
                0 => {
                    let error = Error {
                        r#type: ErrorType::NoMatch,
                        level: ErrorLevel::Error,
                        msg: format!("Account has no positions with cost {}.", &cost_literal),
                        src: posting.src.clone(),
                    };
                    errors.push(error);
                    PostResult::Fail
                }
                1 => {
                    let (unit_cost, holding_number) = candidates[0];
                    let unit_cost = unit_cost.as_ref().unwrap();
                    if p_amount.number.abs() > holding_number.abs() {
                        let error = Error {
                            r#type: ErrorType::NoMatch,
                            level: ErrorLevel::Error,
                            msg: format!(
                                "Account only has {} {} {}.",
                                holding_number, p_amount.currency, unit_cost
                            ),
                            src: posting.src.clone(),
                        };
                        errors.push(error);
                        PostResult::Fail
                    } else {
                        *per_currency_change
                            .entry(unit_cost.amount.currency.to_string())
                            .or_default() += unit_cost.amount.number * p_amount.number;
                        *pending_change.entry(Some(unit_cost.clone())).or_default() +=
                            p_amount.number;
                        let PostingDraft {
                            account,
                            amount: _,
                            cost: _,
                            price,
                            meta,
                            src,
                        } = posting;
                        let valid_posting = Posting {
                            account,
                            amount: p_amount.clone(),
                            cost: Some(unit_cost.clone()),
                            price,
                            meta,
                            src,
                        };
                        PostResult::Success(valid_posting)
                    }
                }
                _ => {
                    let error = Error {
                        r#type: ErrorType::NoMatch,
                        level: ErrorLevel::Error,
                        msg: format!(
                            "Account has multiple positions with cost {}.",
                            &cost_literal
                        ),
                        src: posting.src.clone(),
                    };
                    errors.push(error);
                    PostResult::Fail
                }
            }
        }
    }
}

fn open_new_position(
    posting: PostingDraft,
    txn_date: Date,
    pending_change: &mut HashMap<Option<UnitCost>, Decimal>,
    per_currency_change: &mut HashMap<String, Decimal>,
) -> PostResult {
    let cost_literal = posting.cost.as_ref().unwrap();
    if let Some(cost_basis) = &cost_literal.basis {
        let p_amount = posting.amount.as_ref().unwrap();
        let unit_cost = match cost_basis {
            CostBasis::Total(total_amount) => {
                *per_currency_change
                    .entry(total_amount.currency.to_string())
                    .or_default() += total_amount.number;
                let unit_cost = UnitCost {
                    amount: total_amount / p_amount.number,
                    date: cost_literal.date.unwrap_or(txn_date),
                };
                *pending_change.entry(Some(unit_cost.clone())).or_default() += p_amount.number;
                unit_cost
            }
            CostBasis::Unit(unit_amount) => {
                *per_currency_change
                    .entry(unit_amount.currency.to_string())
                    .or_default() += unit_amount.number * p_amount.number;
                let unit_cost = UnitCost {
                    amount: unit_amount.clone(),
                    date: cost_literal.date.unwrap_or(txn_date),
                };
                *pending_change.entry(Some(unit_cost.clone())).or_default() += p_amount.number;
                unit_cost
            }
        };
        let PostingDraft {
            account,
            amount,
            cost: _,
            price,
            meta,
            src,
        } = posting;
        let valid_posting = Posting {
            account,
            amount: amount.unwrap(),
            cost: Some(unit_cost),
            price,
            meta,
            src,
        };
        PostResult::Success(valid_posting)
    } else {
        PostResult::NeedInfer(posting)
    }
}

fn posting_flow(
    posting: PostingDraft,
    txn_date: Date,
    running_balance: &BalanceSheet,
    balance_change: &mut BalanceSheet,
    per_currency_change: &mut HashMap<String, Decimal>,
    errors: &mut Vec<Error>,
) -> PostResult {
    if posting.amount.is_none() {
        return PostResult::NeedInfer(posting);
    }
    let p_amount = posting.amount.as_ref().unwrap();
    let running_balance = running_balance
        .get(&posting.account)
        .and_then(|m| m.get(&p_amount.currency));
    let pending_change = balance_change
        .entry(posting.account.clone())
        .or_insert(HashMap::new())
        .entry(p_amount.currency.clone())
        .or_insert(HashMap::new());
    if let Some(_) = &posting.cost {
        if is_opening_new(p_amount.number, running_balance) {
            open_new_position(posting, txn_date, pending_change, per_currency_change)
        } else {
            close_position(
                posting,
                running_balance,
                pending_change,
                per_currency_change,
                errors,
            )
        }
    } else {
        let (number, currency) = match &posting.price {
            None => (p_amount.number, &p_amount.currency),
            Some(Price::Total(total_amount)) => {
                if p_amount.number.is_sign_negative() {
                    (-total_amount.number, &total_amount.currency)
                } else {
                    (total_amount.number, &total_amount.currency)
                }
            }
            Some(Price::Unit(unit_price)) => {
                (p_amount.number * unit_price.number, &unit_price.currency)
            }
        };
        *per_currency_change.entry(currency.to_string()).or_default() += number;
        *pending_change.entry(None).or_default() += p_amount.number;
        let PostingDraft {
            account,
            amount,
            cost: _,
            price,
            meta,
            src,
        } = posting;
        let valid_posting = Posting {
            account,
            amount: amount.unwrap(),
            cost: None,
            price,
            meta,
            src,
        };
        PostResult::Success(valid_posting)
    }
}

fn complete_posting(
    incomplete: Option<PostingDraft>,
    not_balanced: Vec<(String, Decimal)>,
    txn_date: Date,
    txn_src: &Source,
    valid_postings: &mut Vec<Posting>,
    balance_change: &mut BalanceSheet,
) -> Result<(), Error> {
    let not_balanced_list = not_balanced
        .iter()
        .map(|(currency, number)| format!("{} {}", number, currency))
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(PostingDraft {
        account,
        amount,
        cost,
        price,
        meta,
        src,
    }) = incomplete
    {
        let pending_change = balance_change.entry(account.clone()).or_default();
        match (amount, cost) {
            (None, _) => {
                for (currency, number) in not_balanced {
                    let valid_posting = Posting {
                        account: account.clone(),
                        amount: Amount {
                            number: -number,
                            currency: currency.clone(),
                        },
                        cost: None,
                        price: None,
                        meta: meta.clone(),
                        src: src.clone(),
                    };
                    *pending_change
                        .entry(currency)
                        .or_default()
                        .entry(None)
                        .or_default() -= number;
                    valid_postings.push(valid_posting);
                }
                Ok(())
            }
            (Some(amount), Some(cost_literal)) => {
                if not_balanced.len() == 1 {
                    let (currency, number) = &not_balanced[0];
                    let cost_date = cost_literal.date.unwrap_or(txn_date);
                    let unit_cost = UnitCost {
                        amount: Amount {
                            number: -number / amount.number,
                            currency: currency.to_string(),
                        },
                        date: cost_date,
                    };
                    *pending_change
                        .entry(amount.currency.clone())
                        .or_default()
                        .entry(Some(unit_cost.clone()))
                        .or_default() += amount.number;
                    let valid_posting = Posting {
                        account,
                        amount,
                        cost: Some(unit_cost),
                        price,
                        meta,
                        src,
                    };
                    valid_postings.push(valid_posting);
                    Ok(())
                } else {
                    let error = Error {
                        msg: format!(
                            "Cannot calculate the cost from multiple unbalanced currencies: {}",
                            not_balanced_list
                        ),
                        src,
                        r#type: ErrorType::Incomplete,
                        level: ErrorLevel::Error,
                    };
                    Err(error)
                }
            }
            _ => unreachable!(),
        }
    } else {
        if not_balanced.len() > 0 {
            let error = Error {
                msg: format!("Transaction not balanced: {}", not_balanced_list),
                r#type: ErrorType::NotBalanced,
                level: ErrorLevel::Error,
                src: txn_src.clone(),
            };
            Err(error)
        } else {
            Ok(())
        }
    }
}

fn check_complete_txn(
    txn: TxnDraft,
    running_balance: &BalanceSheet,
    errors: &mut Vec<Error>,
    tolerances: &HashMap<&str, Decimal>,
) -> Option<(Vec<Transaction>, BalanceSheet)> {
    let mut balance_change = BalanceSheet::new();
    let mut per_currency_change = HashMap::new();
    let TxnDraft {
        date,
        flag,
        payee,
        narration,
        links,
        tags,
        meta,
        postings,
        src,
    } = txn;

    let mut incomplete: Option<PostingDraft> = None;
    let mut valid_postings = Vec::new();
    for posting in postings {
        match posting_flow(
            posting,
            date,
            running_balance,
            &mut balance_change,
            &mut per_currency_change,
            errors,
        ) {
            PostResult::Fail => return None,
            PostResult::Expanded(valid_posting_vec) => valid_postings.extend(valid_posting_vec),
            PostResult::None => {}
            PostResult::Success(valid_posting) => valid_postings.push(valid_posting),
            PostResult::NeedInfer(posting) => {
                if incomplete.is_some() {
                    let error = Error {
                        msg: "Cannot infer the amounts for two posts".to_string(),
                        src: posting.src.clone(),
                        r#type: ErrorType::Incomplete,
                        level: ErrorLevel::Error,
                    };
                    errors.push(error);
                    return None;
                } else {
                    incomplete = Some(posting)
                }
            }
        }
    }
    let not_balanced = per_currency_change
        .into_iter()
        .filter(|(currency, number)| !equal_within(*number, Decimal::zero(), currency, tolerances))
        .collect::<Vec<_>>();
    match complete_posting(
        incomplete,
        not_balanced,
        date,
        &src,
        &mut valid_postings,
        &mut balance_change,
    ) {
        Ok(()) => {}
        Err(e) => {
            errors.push(e);
            return None;
        }
    }
    let valid_txn = Transaction {
        date,
        flag,
        payee,
        narration,
        links,
        tags,
        meta,
        postings: valid_postings,
        src,
    };
    Some((vec![valid_txn], balance_change))
}

fn merge_balance(running_balance: &mut BalanceSheet, changes: BalanceSheet) {
    for (account, account_change) in changes {
        let account_bal = running_balance.entry(account).or_default();
        for (currency, currency_change) in account_change {
            let currency_bal = account_bal.entry(currency).or_default();
            for (cost, cost_change) in currency_change {
                *currency_bal.entry(cost).or_default() += cost_change;
            }
        }
    }
}

const TOLERANCE_KEY_DEFAULT: &str = ";";

fn extract_tolerance<'c>(
    commodities: &'c HashMap<String, (HashMap<String, (String, Source)>, Source)>,
    options: &HashMap<String, (String, Source)>,
    errors: &mut Vec<Error>,
) -> HashMap<&'c str, Decimal> {
    let mut tolerances = HashMap::new();
    for (currency, (meta, _)) in commodities.iter() {
        if let Some((num_str, src)) = meta.get("tolerance") {
            match parse_decimal(num_str, src) {
                Ok(num) => {
                    tolerances.insert(currency.as_str(), num.abs());
                }
                Err(err) => errors.push(err),
            };
        }
    }
    if let Some((num_str, src)) = options.get(OPTION_DEFAULT_TOLERANCE) {
        match parse_decimal(num_str, src) {
            Ok(num) => {
                tolerances.insert(TOLERANCE_KEY_DEFAULT, num.abs());
            }
            Err(err) => errors.push(err),
        }
    } else {
        let default_tolerance = Decimal::new(6, 3);
        tolerances.insert(TOLERANCE_KEY_DEFAULT, default_tolerance);
    }
    tolerances
}

fn equal_within(
    lhs: Decimal,
    rhs: Decimal,
    currency: &str,
    tolerances: &HashMap<&str, Decimal>,
) -> bool {
    if lhs == rhs {
        true
    } else {
        let tolerance = tolerances
            .get(currency)
            .unwrap_or(tolerances.get(TOLERANCE_KEY_DEFAULT).unwrap());
        if (lhs - rhs).abs() < *tolerance {
            true
        } else {
            false
        }
    }
}

struct PadFromInfo {
    from: Account,
    currencies: HashSet<Currency>,
    index: usize,
}

fn try_padding(
    dest_account: &Account,
    pad_number: Decimal,
    currency: &String,
    pad_from: &mut HashMap<Account, PadFromInfo>,
    valid_txns: &mut Vec<Transaction>,
    valid_accounts: &HashMap<Account, AccountInfo>,
    balance_src: &Source,
) -> Result<Account, Option<Error>> {
    if let Some(info) = pad_from.get_mut(dest_account) {
        let from_account_currency_set = &valid_accounts.get(&info.from).unwrap().currencies;
        if from_account_currency_set.len() > 0 && !from_account_currency_set.contains(currency) {
            let error = Error {
                msg: format!("Account {} cannot hold {}.", &info.from, currency),
                level: ErrorLevel::Error,
                r#type: ErrorType::Account,
                src: balance_src.clone(),
            };
            return Err(Some(error));
        }
        if info.currencies.insert(currency.clone()) {
            let pad_place_holder = &mut valid_txns[info.index];
            pad_place_holder.postings.push(Posting {
                account: dest_account.clone(),
                amount: Amount {
                    number: pad_number,
                    currency: currency.clone(),
                },
                cost: None,
                price: None,
                meta: HashMap::new(),
                src: balance_src.clone(),
            });
            pad_place_holder.postings.push(Posting {
                account: info.from.clone(),
                amount: Amount {
                    number: -pad_number,
                    currency: currency.clone(),
                },
                cost: None,
                price: None,
                meta: HashMap::new(),
                src: balance_src.clone(),
            });
            Ok(info.from.clone())
        } else {
            Err(None)
        }
    } else {
        Err(None)
    }
}

fn check_balance(
    txn: TxnDraft,
    running_balance: &mut BalanceSheet,
    errors: &mut Vec<Error>,
    tolerances: &HashMap<&str, Decimal>,
    pad_from: &mut HashMap<Account, PadFromInfo>,
    valid_txns: &mut Vec<Transaction>,
    valid_accounts: &HashMap<Account, AccountInfo>,
) -> Option<Transaction> {
    let mut valid_postings: Vec<Posting> = Vec::new();
    let TxnDraft {
        date,
        flag,
        payee,
        narration,
        links,
        tags,
        meta,
        postings,
        src,
    } = txn;
    for posting in postings {
        if posting.cost.is_some() || posting.price.is_some() {
            errors.push(Error {
                level: ErrorLevel::Error,
                r#type: ErrorType::Syntax,
                msg: "Balance directives only check aggregate amount.".to_string(),
                src: posting.src.clone(),
            });
            continue;
        }
        if let Some(p_amount) = posting.amount.as_ref() {
            let holding_total: Decimal = running_balance
                .get(&posting.account)
                .and_then(|currencies| currencies.get(&p_amount.currency))
                .map(|position| position.values().sum())
                .unwrap_or(Decimal::zero());

            if equal_within(
                holding_total,
                p_amount.number,
                &p_amount.currency,
                tolerances,
            ) || match try_padding(
                &posting.account,
                p_amount.number - holding_total,
                &p_amount.currency,
                pad_from,
                valid_txns,
                valid_accounts,
                &posting.src,
            ) {
                Ok(account_from) => {
                    let pad_amount = p_amount.number - holding_total;
                    *running_balance
                        .entry(posting.account.clone())
                        .or_default()
                        .entry(p_amount.currency.clone())
                        .or_default()
                        .entry(None)
                        .or_default() += pad_amount;
                    *running_balance
                        .entry(account_from)
                        .or_default()
                        .entry(p_amount.currency.clone())
                        .or_default()
                        .entry(None)
                        .or_default() -= pad_amount;
                    true
                }
                Err(None) => {
                    let assert_err = Error {
                        level: ErrorLevel::Error,
                        r#type: ErrorType::NotBalanced,
                        msg: format!(
                            "Failed assertion: {} != {} {}.",
                            &p_amount, holding_total, &p_amount.currency
                        ),
                        src: posting.src.clone(),
                    };
                    errors.push(assert_err);
                    false
                }
                Err(Some(error)) => {
                    errors.push(error);
                    false
                }
            } {
                let PostingDraft {
                    account,
                    amount: _,
                    cost: _,
                    price: _,
                    meta,
                    src,
                } = posting;
                valid_postings.push(Posting {
                    account,
                    amount: p_amount.clone(),
                    cost: None,
                    price: None,
                    meta,
                    src,
                });
            }
        } else {
            let error = Error {
                level: ErrorLevel::Error,
                r#type: ErrorType::Incomplete,
                msg: "Missing amount.".to_string(),
                src: posting.src.clone(),
            };
            errors.push(error);
        }
    }
    if valid_postings.len() > 0 {
        let valid_txn = Transaction {
            date,
            flag,
            payee,
            narration,
            links,
            tags,
            meta,
            postings: valid_postings,
            src,
        };
        Some(valid_txn)
    } else {
        None
    }
}

impl LedgerDraft {
    /// Consuming `self`, returns a [`Ledger`] and the errors encountered
    /// during verifying accounts, calculating missing amounts or omitted cost
    /// basis information, checking `balance` assertions, and completing `pad`
    /// directives. If a directive causes an error with [`ErrorLevel::Error`],
    /// it is dropped.
    /// In this case, the returned [`Ledger`]
    /// contains a subset of the information in `self`.

    pub fn into_ledger(self) -> (Ledger, Vec<Error>) {
        let LedgerDraft {
            accounts,
            commodities,
            mut txns,
            options,
            events,
        } = self;
        let mut errors = Vec::new();
        let valid_accounts = check_accounts(accounts, &mut errors);
        let tolerances = extract_tolerance(&commodities, &options, &mut errors);
        let mut valid_txns: Vec<Transaction> = Vec::new();
        let mut running_balance = BalanceSheet::new();
        let mut pad_from: HashMap<Account, PadFromInfo> = HashMap::new();
        let mut pad_to: HashMap<Account, HashSet<Account>> = HashMap::new();
        let option_balance_at_day_end: bool = options
            .get(OPTION_BALANCE_AT_DAY_END)
            .map(|v| &v.0)
            .and_then(|s| s.parse().ok())
            .unwrap_or(false);
        if option_balance_at_day_end {
            txns.sort_by_key(|t| (t.date, t.flag));
        } else {
            txns.sort_by_key(|t| (t.date, (t.flag as u8 + 1) % 4));
        }
        for txn in txns {
            let mut valid = true;
            for posting in txn.postings.iter() {
                if let Err(msg) = check_posting(posting, txn.date, &valid_accounts) {
                    errors.push(Error {
                        msg: msg,
                        src: posting.src.clone(),
                        level: ErrorLevel::Error,
                        r#type: ErrorType::Account,
                    });
                    valid = false;
                }
            }
            if !valid {
                continue;
            }

            match txn.flag {
                TxnFlag::Balance => {
                    for posting in txn.postings.iter() {
                        if let Some(set) = pad_to.remove(&posting.account) {
                            for dest_account in set {
                                pad_from.remove(&dest_account);
                            }
                        }
                    }
                    if let Some(valid_txn) = check_balance(
                        txn,
                        &mut running_balance,
                        &mut errors,
                        &tolerances,
                        &mut pad_from,
                        &mut valid_txns,
                        &valid_accounts,
                    ) {
                        valid_txns.push(valid_txn);
                    }
                }
                TxnFlag::Pending | TxnFlag::Posted => {
                    if let Some((valid_txn_vec, changes)) =
                        check_complete_txn(txn, &running_balance, &mut errors, &tolerances)
                    {
                        valid_txns.extend(valid_txn_vec);
                        merge_balance(&mut running_balance, changes);
                    }
                }
                TxnFlag::Pad => {
                    let TxnDraft {
                        date,
                        flag,
                        payee: _,
                        narration: _,
                        links,
                        tags,
                        meta,
                        postings,
                        src,
                    } = txn;
                    if postings.len() == 2 {
                        let pad_placeholder = Transaction {
                            date,
                            flag,
                            payee: String::new(),
                            narration: format!(
                                "Pad {} from {}",
                                &postings[0].account, &postings[1].account
                            ),
                            links,
                            tags,
                            meta,
                            postings: Vec::new(),
                            src,
                        };
                        pad_from.insert(
                            postings[0].account.clone(),
                            PadFromInfo {
                                from: postings[1].account.clone(),
                                currencies: HashSet::new(),
                                index: valid_txns.len(),
                            },
                        );
                        pad_to
                            .entry(postings[1].account.clone())
                            .or_default()
                            .insert(postings[0].account.clone());
                        valid_txns.push(pad_placeholder);
                    } else {
                        let error = Error {
                            msg: "Invalid syntax: Pad must contains two accounts.".to_string(),
                            level: ErrorLevel::Error,
                            r#type: ErrorType::Syntax,
                            src,
                        };
                        errors.push(error);
                    }
                }
            }
        }
        let ledger = Ledger {
            accounts: valid_accounts,
            commodities,
            txns: valid_txns,
            options,
            events,
            balance_sheet: running_balance,
        };
        (ledger, errors)
    }
}

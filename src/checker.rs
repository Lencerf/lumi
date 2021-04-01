use std::collections::HashMap;

use crate::{
    parse::{AccountInfoDraft, LedgerDraft, PostingDraft},
    Account, AccountInfo, Amount, BalanceSheet, Date, Error, ErrorLevel, ErrorType, Ledger,
    Transaction,
};

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

impl LedgerDraft {
    pub fn to_ledger(self, errors: &mut Vec<Error>) -> Ledger {
        let LedgerDraft {
            accounts,
            commodities,
            txns,
            options,
            events,
        } = self;
        let valid_accounts = check_accounts(accounts, errors);

        let valid_txns: Vec<Transaction> = Vec::new();
        let running_balance = BalanceSheet::new();
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

            // TODO: check if the transaction is balanced.
        }
        let ledger = Ledger {
            accounts: valid_accounts,
            commodities,
            txns: valid_txns,
            options,
            events,
            balance_sheet: running_balance,
        };
        ledger
    }
}

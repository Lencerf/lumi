//! Read-only queries over a [`Ledger`] that back the user interfaces: account
//! hierarchies with their balances, holdings, and filtered, paginated journals.
//!
//! Every function here is a pure function of the ledger and the request, so
//! the same code answers an HTTP request in `lumi serve` and a direct call
//! from a native application.

use std::collections::{HashMap, HashSet};

use chrono::Datelike;
use rust_decimal::Decimal;

use crate::web::{
    Completions, DEFAULT_ENTRIES_PER_PAGE, FilterOptions, JournalItem, Position, TrieNode,
    TrieOptions, TrieTable, TrieTableRow,
};
use crate::{BalanceSheet, Ledger, Transaction, TxnFlag};

/// Flattens a balance sheet into the list of positions held by each account.
///
/// The order of the positions within an account is unspecified.
pub fn positions(sheet: &BalanceSheet) -> HashMap<String, Vec<Position>> {
    let mut result = HashMap::new();
    for (account, account_map) in sheet {
        let list = result.entry(account.to_string()).or_insert_with(Vec::new);
        for (currency, currency_map) in account_map {
            for (cost, number) in currency_map {
                list.push(Position {
                    number: *number,
                    currency: currency.clone(),
                    cost: cost.clone(),
                })
            }
        }
    }
    result
}

/// Builds the account hierarchy below `root_account` (e.g. `Assets`), where
/// each node carries the balances of all the accounts below it, aggregated by
/// currency. Positions held at cost are counted in the cost currency.
///
/// Returns the tree, whose root holds `root_account` as its only child, and
/// the set of currencies that appear in it.
pub fn trie<'s>(
    ledger: &'s Ledger,
    root_account: &str,
    options: &TrieOptions,
) -> (TrieNode<&'s str>, HashSet<&'s str>) {
    let show_closed = options.show_closed.unwrap_or(false);
    let mut root_node = TrieNode::default();
    let mut currencies = HashSet::new();
    for (account, account_map) in ledger.balance_sheet() {
        if ledger.accounts()[account].close().is_some() && !show_closed {
            continue;
        }
        let mut parts = account.split(':');
        if parts.next() != Some(root_account) {
            continue;
        }
        let mut account_holdings: HashMap<&'s str, Decimal> = HashMap::new();
        for (currency, cost_map) in account_map {
            for (cost, number) in cost_map {
                if number.is_zero() {
                    continue;
                }
                if let Some(unit_cost) = cost {
                    let cost_currency = unit_cost.amount.currency.as_str();
                    *account_holdings.entry(cost_currency).or_default() +=
                        unit_cost.amount.number * number;
                    currencies.insert(cost_currency);
                } else {
                    *account_holdings.entry(currency.as_str()).or_default() += number;
                    currencies.insert(currency.as_str());
                }
            }
        }
        let mut leaf_node = &mut root_node;
        for key in account.split(':') {
            leaf_node = leaf_node.nodes.entry(key).or_default();
            for (currency, number) in account_holdings.iter() {
                *leaf_node.numbers.entry(currency).or_default() += number;
            }
        }
    }
    (root_node, currencies)
}

/// Lays out the hierarchy from [`trie`] as table rows in depth-first order,
/// with one formatted column per currency (sorted) and blank cells for zero
/// balances. Returns [`None`] if no account starts with `root_account`.
pub fn trie_table<'s>(
    ledger: &'s Ledger,
    root_account: &str,
    options: &TrieOptions,
) -> Option<TrieTable<&'s str>> {
    let (trie, currencies) = trie(ledger, root_account, options);
    let (name, node) = trie.nodes.get_key_value(root_account)?;
    let mut currencies: Vec<_> = currencies.into_iter().collect();
    currencies.sort_unstable();
    let mut rows = Vec::new();
    push_trie_rows(name, 0, node, &currencies, &mut rows);
    Some(TrieTable { rows, currencies })
}

fn push_trie_rows<'s>(
    name: &'s str,
    level: usize,
    node: &TrieNode<&'s str>,
    currencies: &[&'s str],
    rows: &mut Vec<TrieTableRow<&'s str>>,
) {
    let numbers = currencies
        .iter()
        .map(|c| {
            let number = node.numbers.get(*c).copied().unwrap_or_default();
            if number.is_zero() {
                String::new()
            } else {
                format!("{:.2}", number)
            }
        })
        .collect();
    rows.push(TrieTableRow {
        level,
        name,
        numbers,
    });
    let mut children: Vec<_> = node.nodes.iter().collect();
    children.sort_by_key(|(name, _)| *name);
    for (name, child) in children {
        push_trie_rows(name, level + 1, child, currencies, rows);
    }
}

fn touches_account(txn: &Transaction, account: &str) -> bool {
    txn.postings()
        .iter()
        .any(|posting| posting.account.starts_with(account))
}

fn contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle_lower.is_ascii() {
        return haystack
            .as_bytes()
            .windows(needle_lower.len())
            .any(|window| window.eq_ignore_ascii_case(needle_lower.as_bytes()));
    }
    haystack.to_lowercase().contains(needle_lower)
}

fn query_terms(query: Option<&str>) -> Vec<String> {
    query
        .map(|q| {
            q.split_whitespace()
                .filter(|term| !term.is_empty())
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default()
}

fn txn_matches_term(txn: &Transaction, term_lower: &str, date_text: &str) -> bool {
    if contains_ignore_case(date_text, term_lower)
        || contains_ignore_case(txn.payee(), term_lower)
        || contains_ignore_case(txn.narration(), term_lower)
        || txn
            .tags()
            .iter()
            .any(|tag| contains_ignore_case(tag, term_lower))
        || txn
            .links()
            .iter()
            .any(|link| contains_ignore_case(link, term_lower))
    {
        return true;
    }

    txn.postings().iter().any(|posting| {
        contains_ignore_case(&posting.account, term_lower)
            || contains_ignore_case(&posting.amount.currency, term_lower)
            || contains_ignore_case(&posting.amount.number.to_string(), term_lower)
            || posting.cost.as_ref().is_some_and(|cost| {
                contains_ignore_case(&cost.amount.currency, term_lower)
                    || contains_ignore_case(&cost.amount.number.to_string(), term_lower)
                    || contains_ignore_case(&cost.date.to_string(), term_lower)
            })
            || posting.price.as_ref().is_some_and(|price| {
                contains_ignore_case(&price.currency, term_lower)
                    || contains_ignore_case(&price.number.to_string(), term_lower)
            })
    })
}

fn txn_matches_query(txn: &Transaction, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let date_text = txn.date().to_string();
    terms
        .iter()
        .all(|term| txn_matches_term(txn, term, &date_text))
}

fn parse_flag_token(token: &str) -> Option<TxnFlag> {
    match token {
        "*" | "txn" | "posted" | "cleared" => Some(TxnFlag::Posted),
        "!" | "?" | "pending" => Some(TxnFlag::Pending),
        "bal" | "balance" => Some(TxnFlag::Balance),
        "pad" => Some(TxnFlag::Pad),
        _ => None,
    }
}

fn parse_flag_filter(flags: Option<&str>) -> Option<HashSet<TxnFlag>> {
    let raw = flags?;
    let set = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter_map(|part| parse_flag_token(&part.to_ascii_lowercase()))
        .collect();
    Some(set)
}

/// Adds the amounts `txn` posts to `account` (and its sub-accounts) to
/// `running_balance`, and returns those changes by currency. Postings held at
/// cost and balance assertions do not change the balance.
fn update_balance<'t>(
    txn: &'t Transaction,
    account: &str,
    running_balance: &mut HashMap<&'t str, Decimal>,
) -> HashMap<&'t str, Decimal> {
    if txn.flag() == TxnFlag::Balance {
        return HashMap::new();
    }
    let mut changes: HashMap<&str, Decimal> = HashMap::new();
    for posting in txn.postings().iter() {
        if posting.cost.is_none() && posting.account.starts_with(account) {
            *changes.entry(posting.amount.currency.as_str()).or_default() += posting.amount.number;
        }
    }
    for (c, n) in changes.iter() {
        *running_balance.entry(c).or_default() += n;
    }
    changes
}

/// One page of a journal: the items on the page and the total number of
/// transactions matching the query across all pages.
pub type JournalPage<'t> = (Vec<JournalItem<&'t str, &'t Transaction>>, usize);

/// Returns one page of the journal and the total number of matching
/// transactions.
///
/// With `account`, only transactions touching that account (or one of its
/// sub-accounts) are listed, and each item carries the changes the
/// transaction makes to the account and its running balance afterwards.
/// `options` can further restrict by account prefix and year, and select the
/// page, the page size and whether the oldest transactions come first.
pub fn journal<'t>(
    ledger: &'t Ledger,
    account: Option<&str>,
    options: &FilterOptions,
) -> JournalPage<'t> {
    let year = options
        .time
        .as_ref()
        .and_then(|time| time.parse::<i32>().ok());
    let terms = query_terms(options.query.as_deref());
    let allowed_flags = parse_flag_filter(options.flags.as_deref());
    let matches = |txn: &Transaction| {
        account.is_none_or(|account| touches_account(txn, account))
            && options
                .account
                .as_deref()
                .is_none_or(|account| touches_account(txn, account))
            && year.is_none_or(|year| txn.date().year() == year)
            && allowed_flags
                .as_ref()
                .is_none_or(|flags| flags.contains(&txn.flag()))
            && txn_matches_query(txn, &terms)
    };
    let txns: Vec<_> = ledger.txns().iter().filter(|txn| matches(txn)).collect();
    let total_number = txns.len();
    let page = std::cmp::max(options.page.unwrap_or(1), 1);
    let entries = std::cmp::max(options.entries.unwrap_or(DEFAULT_ENTRIES_PER_PAGE), 1);
    let old_first = options.old_first.unwrap_or(false);
    if (page - 1) * entries >= txns.len() {
        return (Vec::new(), total_number);
    }
    let num_skip = if old_first {
        (page - 1) * entries
    } else if page * entries >= txns.len() {
        0
    } else {
        txns.len() - page * entries
    };
    let mut running_balance: HashMap<&str, Decimal> = HashMap::new();
    if let Some(account) = account {
        for txn in txns.iter().take(num_skip) {
            update_balance(txn, account, &mut running_balance);
        }
    }
    let num_take = if old_first {
        std::cmp::min(entries, txns.len() - entries * (page - 1))
    } else {
        (txns.len() - entries * (page - 1)) - num_skip
    };
    let mut items: Vec<_> = txns
        .into_iter()
        .skip(num_skip)
        .take(num_take)
        .map(|txn| match account {
            Some(account) => {
                let changes = update_balance(txn, account, &mut running_balance);
                JournalItem {
                    txn,
                    balance: running_balance.clone(),
                    changes,
                }
            }
            None => JournalItem {
                txn,
                balance: HashMap::new(),
                changes: HashMap::new(),
            },
        })
        .collect();
    if !old_first {
        items.reverse();
    }
    (items, total_number)
}

/// Extracts completion candidates from `ledger` for editing entries: open
/// accounts, currencies, payees, narrations, per-payee narrations, tags, and
/// links.
pub fn completions(ledger: &Ledger) -> Completions {
    let mut accounts: Vec<String> = ledger
        .accounts()
        .iter()
        .filter(|(_, info)| info.close().is_none())
        .map(|(account, _)| account.as_ref().clone())
        .collect();
    accounts.sort_unstable();

    let mut currencies = Vec::new();
    let mut seen_currencies = HashSet::new();
    let mut push_currency = |currency: &str| {
        if !currency.is_empty() && seen_currencies.insert(currency.to_string()) {
            currencies.push(currency.to_string());
        }
    };

    if let Some((operating, _)) = ledger.options().get("operating_currency") {
        push_currency(operating);
    }

    let mut payees = Vec::new();
    let mut seen_payees = HashSet::new();
    let mut narrations = Vec::new();
    let mut seen_narrations = HashSet::new();
    let mut payee_narrations: HashMap<String, Vec<String>> = HashMap::new();
    let mut tags = Vec::new();
    let mut seen_tags = HashSet::new();
    let mut links = Vec::new();
    let mut seen_links = HashSet::new();

    for txn in ledger.txns().iter().rev() {
        let payee = txn.payee();
        let narration = txn.narration();
        if !payee.is_empty() && seen_payees.insert(payee.as_str()) {
            payees.push(payee.clone());
        }
        if !narration.is_empty() && seen_narrations.insert(narration.as_str()) {
            narrations.push(narration.clone());
        }
        if !payee.is_empty() && !narration.is_empty() {
            let list = payee_narrations.entry(payee.clone()).or_default();
            if !list.iter().any(|existing| existing == narration) {
                list.push(narration.clone());
            }
        }
        for tag in txn.tags() {
            if !tag.is_empty() && seen_tags.insert(tag.as_str()) {
                tags.push(tag.clone());
            }
        }
        for link in txn.links() {
            if !link.is_empty() && seen_links.insert(link.as_str()) {
                links.push(link.clone());
            }
        }
        for posting in txn.postings() {
            push_currency(&posting.amount.currency);
            if let Some(cost) = &posting.cost {
                push_currency(&cost.amount.currency);
            }
            if let Some(price) = &posting.price {
                push_currency(&price.currency);
            }
        }
    }

    let mut declared_currencies: Vec<&str> = ledger
        .commodities()
        .keys()
        .map(String::as_str)
        .chain(
            ledger
                .accounts()
                .values()
                .flat_map(|info| info.currencies().iter().map(String::as_str)),
        )
        .collect();
    declared_currencies.sort_unstable();
    for currency in declared_currencies {
        push_currency(currency);
    }

    Completions {
        accounts,
        currencies,
        payees,
        narrations,
        payee_narrations,
        tags,
        links,
    }
}

#[cfg(test)]
#[path = "query_test.rs"]
mod tests;

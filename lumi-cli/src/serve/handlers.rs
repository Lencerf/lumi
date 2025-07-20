use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Datelike;
use lumi::web::{
    FilterOptions, JournalItem, Position, RefreshTime, TrieNode, TrieOptions, TrieTable,
    TrieTableRow,
};
use lumi::{BalanceSheet, Ledger, Transaction, TxnFlag};
use rust_decimal::Decimal;
use tokio::sync::RwLock;

use crate::serve::LedgerData;

pub async fn get_refresh(
    State(path): State<Arc<str>>,
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> Response {
    let (new_ledger, new_errors) = Ledger::from_file(&path);
    let mut data = data.write().await;
    data.ledger = new_ledger;
    data.errors = new_errors;
    let timestamp = chrono::Utc::now().timestamp();
    let reply = RefreshTime { timestamp };
    log::info!("Ledger refreshed: {}", timestamp);
    Json(reply).into_response()
}

fn balance_sheet_to_list(sheet: &BalanceSheet) -> HashMap<String, Vec<Position>> {
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

fn build_trie_table_helper<'s, 'r: 's>(
    root: &'r str,
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
    let row = TrieTableRow {
        level,
        name: root,
        numbers,
    };
    rows.push(row);
    let mut sorted_kv: Vec<_> = node.nodes.iter().collect();
    sorted_kv.sort_by_key(|kv| kv.0);
    for (account, sub_trie) in sorted_kv {
        build_trie_table_helper(account, level + 1, sub_trie, currencies, rows);
    }
}

fn build_trie_table<'s, 'r: 's>(
    ledger: &'s Ledger,
    root_account: &'r str,
    options: TrieOptions,
) -> Option<TrieTable<&'s str>> {
    let (trie, currencies) = build_trie(ledger, root_account, options);
    if let Some(node) = trie.nodes.get(root_account) {
        let mut currencies: Vec<_> = currencies.into_iter().collect();
        currencies.sort_unstable();
        let mut rows = Vec::new();
        build_trie_table_helper(root_account, 0, node, &currencies, &mut rows);
        Some(TrieTable { rows, currencies })
    } else {
        None
    }
}

pub fn build_trie<'s>(
    ledger: &'s Ledger,
    root_account: &str,
    options: TrieOptions,
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

pub async fn get_trie(
    Path(account): Path<String>,
    Query(options): Query<TrieOptions>,
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> Response {
    let ledger = &data.read().await.ledger;
    let Some(trie_table) = build_trie_table(&ledger, &account, options) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(&trie_table).into_response()
}

pub async fn get_errors(Extension(data): Extension<Arc<RwLock<LedgerData>>>) -> Response {
    let errors = &data.read().await.errors;
    (
        [
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                header::HeaderValue::from_static("*"),
            ),
            (
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                header::HeaderValue::from_static("true"),
            )
        ]
        ,
        Json(errors)
    ).into_response()
    // Json(errors).into_response()
}

pub async fn get_balances(
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> impl IntoResponse {
    let ledger = &data.read().await.ledger;
    Json(balance_sheet_to_list(ledger.balance_sheet()))
}

fn filter_account(txn: &Transaction, account: &str) -> bool {
    for posting in txn.postings() {
        if posting.account.starts_with(account) {
            return true;
        }
    }
    false
}

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
        if posting.cost.is_none() && posting.account.starts_with(&account) {
            *changes.entry(posting.amount.currency.as_str()).or_default() += posting.amount.number;
        }
    }
    for (c, n) in changes.iter() {
        *running_balance.entry(c).or_default() += n;
    }
    changes
}

pub async fn get_journal(
    Query(options): Query<FilterOptions>,
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> Response {
    account_journal(None, options, data).await
}

pub async fn get_account(
    Path(account): Path<String>,
    Query(options): Query<FilterOptions>,
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> Response {
    account_journal(Some(account), options, data).await
}

async fn account_journal(
    account: Option<String>,
    options: FilterOptions,
    data: Arc<RwLock<LedgerData>>,
) -> Response {
    let ledger = &data.read().await.ledger;
    let mut filters: Vec<Box<dyn Fn(&Transaction) -> bool>> = Vec::new();
    if let Some(ref account) = account {
        filters.push(Box::new(move |txn: &Transaction| {
            filter_account(txn, account)
        }));
    }
    if let Some(account) = &options.account {
        filters.push(Box::new(move |txn: &Transaction| {
            filter_account(txn, account)
        }));
    };
    if let Some(time) = &options.time {
        if let Ok(year) = time.parse::<i32>() {
            filters.push(Box::new(move |txn: &Transaction| txn.date().year() == year));
        }
    }
    let txns: Vec<_> = ledger
        .txns()
        .iter()
        .filter(|t| {
            for filter in filters.iter() {
                if !filter(t) {
                    return false;
                }
            }
            true
        })
        .collect();
    let total_number = txns.len();
    let page = std::cmp::max(options.page.unwrap_or(1), 1);
    let entries = std::cmp::max(options.entries.unwrap_or(50), 1);
    let old_first = options.old_first.unwrap_or(false);
    if (page - 1) * entries >= txns.len() {
        let empty: [u8; 0] = [];
        Json((empty, total_number)).into_response()
    } else {
        let num_skip = if old_first {
            (page - 1) * entries
        } else if page * entries >= txns.len() {
            0
        } else {
            txns.len() - page * entries
        };
        let mut running_balance: HashMap<&str, Decimal> = HashMap::new();
        if let Some(ref account) = account {
            for txn in txns.iter().take(num_skip) {
                let _ = update_balance(txn, account, &mut running_balance);
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
            .map(|txn| {
                if let Some(ref account) = account {
                    let changes = update_balance(txn, account, &mut running_balance);
                    JournalItem {
                        txn,
                        balance: running_balance.clone(),
                        changes,
                    }
                } else {
                    JournalItem {
                        txn,
                        balance: HashMap::new(),
                        changes: HashMap::new(),
                    }
                }
            })
            .collect();
        if !old_first {
            items.reverse();
        }
        Json(&(items, total_number)).into_response()
    }
}

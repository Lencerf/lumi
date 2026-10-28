use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use lumi::web::{FilterOptions, RefreshTime, TrieOptions};
use lumi::{Ledger, query};
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

pub async fn get_trie(
    Path(account): Path<String>,
    Query(options): Query<TrieOptions>,
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> Response {
    let ledger = &data.read().await.ledger;
    let Some(trie_table) = query::trie_table(ledger, &account, &options) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(&trie_table).into_response()
}

pub async fn get_errors(Extension(data): Extension<Arc<RwLock<LedgerData>>>) -> Response {
    let errors = &data.read().await.errors;
    Json(errors).into_response()
}

pub async fn get_balances(
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> impl IntoResponse {
    let ledger = &data.read().await.ledger;
    Json(query::positions(ledger.balance_sheet()))
}

pub async fn get_journal(
    Query(options): Query<FilterOptions>,
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> Response {
    let ledger = &data.read().await.ledger;
    Json(&query::journal(ledger, None, &options)).into_response()
}

pub async fn get_account(
    Path(account): Path<String>,
    Query(options): Query<FilterOptions>,
    Extension(data): Extension<Arc<RwLock<LedgerData>>>,
) -> Response {
    let ledger = &data.read().await.ledger;
    Json(&query::journal(ledger, Some(&account), &options)).into_response()
}

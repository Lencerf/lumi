use super::handlers;
use lumi::web::{FilterOptions, TrieOptions};
use lumi::{Error, Ledger};
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::Filter;

fn with_ledger(
    ledger: Arc<RwLock<Ledger>>,
) -> impl Filter<Extract = (Arc<RwLock<Ledger>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || ledger.clone())
}

fn with_errors(
    errors: Arc<RwLock<Vec<Error>>>,
) -> impl Filter<Extract = (Arc<RwLock<Vec<Error>>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || errors.clone())
}

fn with_path(
    path: String,
) -> impl Filter<Extract = (String,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || path.clone())
}

pub fn refresh(
    ledger: Arc<RwLock<Ledger>>,
    errors: Arc<RwLock<Vec<Error>>>,
    path: String,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::path("refresh")
        .and(warp::get())
        .and(with_ledger(ledger))
        .and(with_errors(errors))
        .and(with_path(path))
        .and_then(handlers::refresh)
}

pub fn ledger_api(
    ledger: Arc<RwLock<Ledger>>,
    errors: Arc<RwLock<Vec<Error>>>,
    path: &str,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::path("api").and(
        refresh(ledger.clone(), errors.clone(), path.to_owned())
            .or(get_balances(ledger.clone()))
            .or(get_journal_all(ledger.clone()))
            .or(get_journal(ledger.clone()))
            .or(get_trie(ledger))
            .or(get_errors(errors)),
    )
}

pub fn get_balances(
    ledger: Arc<RwLock<Ledger>>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::path("balances")
        .and(warp::get())
        .and(with_ledger(ledger))
        .and_then(handlers::balances)
}

pub fn get_errors(
    errors: Arc<RwLock<Vec<Error>>>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::path("errors")
        .and(warp::get())
        .and(with_errors(errors))
        .and_then(handlers::errors)
}

pub fn get_trie(
    ledger: Arc<RwLock<Ledger>>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::get()
        .and(warp::path("trie"))
        .and(warp::path::param())
        .and(warp::query::<TrieOptions>())
        .and(with_ledger(ledger))
        .and_then(handlers::trie)
}

pub fn get_journal(
    ledger: Arc<RwLock<Ledger>>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::get()
        .and(warp::path("account"))
        .and(warp::path::param())
        .and(warp::query::<FilterOptions>())
        .and(with_ledger(ledger))
        .and_then(|account, options, ledger| {
            handlers::account_journal(Some(account), options, ledger)
        })
}

pub fn get_journal_all(
    ledger: Arc<RwLock<Ledger>>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::get()
        .and(warp::path("journal"))
        .and(warp::path::end())
        .and(warp::query::<FilterOptions>())
        .and(with_ledger(ledger))
        .and_then(|options, ledger| handlers::account_journal(None, options, ledger))
}

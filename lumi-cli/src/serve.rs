use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use include_dir::{Dir, include_dir};
use lumi::{Error, Ledger};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::{RwLock, oneshot};

mod handlers;

static WEB_DIR: Dir = include_dir!("$OUT_DIR/site");

async fn file(path: Option<Path<String>>) -> Response {
    let pages = [
        "index.html",
        "errors",
        "holdings",
        "account",
        "journal",
        "income",
        "balance_sheet",
    ];
    let key = if let Some(Path(p)) = &path {
        let page = if let Some((dir, _)) = p.split_once('/') {
            dir
        } else {
            p.as_str()
        };
        if pages.contains(&page) {
            "index.html"
        } else {
            p.as_str()
        }
    } else {
        "index.html"
    };

    let Some(f) = WEB_DIR.get_file(key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let contents = f.contents();

    let Some((_, suffix)) = key.rsplit_once('.') else {
        return Bytes::from(contents).into_response();
    };
    let mime = match suffix {
        "html" => mime::TEXT_HTML_UTF_8.as_ref(),
        "css" => mime::TEXT_CSS_UTF_8.as_ref(),
        "js" => mime::APPLICATION_JAVASCRIPT_UTF_8.as_ref(),
        "wasm" => "application/wasm",
        _ => mime::OCTET_STREAM.as_str(),
    };

    (
        [(header::CONTENT_TYPE, HeaderValue::from_static(mime))],
        contents,
    )
        .into_response()
}

pub struct LedgerData {
    ledger: Ledger,
    errors: Vec<Error>,
}

pub async fn serve(
    addr: String,
    path: &str,
    ledger: Ledger,
    errors: Vec<lumi::Error>,
) -> std::io::Result<()> {
    pretty_env_logger::init();

    let state = Arc::new(RwLock::new(LedgerData { ledger, errors }));
    let src_path = Arc::<str>::from(path);

    let api_routes = Router::new()
        .without_v07_checks()
        .route("/balances", get(handlers::get_balances))
        .route("/errors", get(handlers::get_errors))
        .route("/trie/{account}", get(handlers::get_trie))
        .route("/journal", get(handlers::get_journal))
        .route("/account/{account}", get(handlers::get_account))
        .route("/refresh", get(handlers::get_refresh))
        .with_state(src_path)
        .layer(Extension(state));

    let app = Router::new()
        .nest("/api", api_routes)
        .route("/", get(file))
        .route("/{*file}", get(file));

    let addr = if addr.is_empty() {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8001))
    } else {
        addr.parse().map_err(|_| std::io::ErrorKind::InvalidInput)?
    };

    let (tx, rx) = oneshot::channel();

    let listener = TcpListener::bind(addr).await?;
    let server = axum::serve(listener, app).with_graceful_shutdown(async {
        rx.await.ok();
    });

    let handle = tokio::task::spawn(async { server.await });
    println!("listening on http://{}", &addr);

    signal::ctrl_c().await?;
    tx.send(()).ok();

    handle.await?
}

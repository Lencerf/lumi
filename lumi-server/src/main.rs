use clap::{clap_app, App};
use headers::{ContentType, HeaderMapExt};
use include_dir::{include_dir, Dir};
use lumi::Ledger;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::signal;
use tokio::sync::{oneshot, RwLock};
use warp::Filter;

mod filters;
mod handlers;

static WEB_DIR: Dir = include_dir!("../lumi-web/dist");

fn get_file(path: &str) -> Option<&'static [u8]> {
    WEB_DIR.get_file(path).map(|f| f.contents)
}

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHOR: &str = env!("CARGO_PKG_AUTHORS");

#[tokio::main]
async fn main() -> std::io::Result<()> {
    pretty_env_logger::init();
    let root_index = warp::path::end().map(|| {
        let index = get_file("index.html").unwrap();
        warp::reply::html(index)
    });

    let pages: HashSet<&str> = [
        "errors",
        "holdings",
        "account",
        "journal",
        "income",
        "balance_sheet",
    ]
    .into_iter()
    .collect();
    let file = warp::path::param().map(move |path: String| {
        if let Some(contents) = get_file(&path) {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let mut resp = warp::reply::Response::new(contents.into());
            resp.headers_mut().typed_insert(ContentType::from(mime));
            resp
        } else if pages.contains(path.as_str()) {
            let index = get_file("index.html").unwrap();
            let mut resp = warp::reply::Response::new(index.into());
            resp.headers_mut().typed_insert(ContentType::html());
            resp
        } else {
            let mut resp = warp::reply::Response::default();
            *resp.status_mut() = warp::http::StatusCode::NOT_FOUND;
            resp
        }
    });
    let get_file = warp::get().and(root_index.or(file));

    let matches = clap_app!(@app(App::new("lumi-server"))
        (version: VERSION)
        (author: AUTHOR)
        (@arg INPUT: +required "Input file")
        (@arg ADDR: -a --("addr") +takes_value "Bind address" )
    )
    .get_matches();
    let path = matches.value_of("INPUT").unwrap();
    let addr: SocketAddr = matches
        .value_of("ADDR")
        .and_then(|addr| addr.parse().ok())
        .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3000));
    let (ledger, errors) = Ledger::from_file(path);
    let api = filters::ledger_api(
        Arc::new(RwLock::new(ledger)),
        Arc::new(RwLock::new(errors)),
        path,
    );

    let routes = api.or(get_file).with(warp::log("lumi-server"));
    let (tx, rx) = oneshot::channel();
    let (_addr, server) = warp::serve(routes).bind_with_graceful_shutdown(addr, async {
        rx.await.ok();
    });
    let handle = tokio::task::spawn(server);

    signal::ctrl_c().await?;
    tx.send(()).ok();

    handle.await?;
    Ok(())
}

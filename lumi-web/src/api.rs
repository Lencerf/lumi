use lumi_server_defs::{FilterOptions, JournalItem, Position, RefreshTime, TrieOptions, TrieTable};
use std::{collections::HashMap, rc::Rc, string::ToString};
use yew::{Component, Context};
use yew_router::history::{BrowserHistory, History};

pub enum FetchState<T> {
    NotStarted,
    Fetching,
    Success(T),
    Failed(anyhow::Error),
}

async fn fetch_json_content<D>(url: String) -> anyhow::Result<D>
where
    D: for<'de> serde::de::Deserialize<'de>,
{
    Ok(reqwest::get(url).await?.json::<D>().await?)
}

fn fetch<C, F, D, M>(ctx: &Context<C>, rel_url: &str, callback: F)
where
    F: Fn(anyhow::Result<D>) -> M + 'static,
    C: Component,
    M: Into<C::Message>,
    D: for<'de> serde::de::Deserialize<'de>,
{
    let location = BrowserHistory::new().location();
    let link = ctx.link();
    let url = format!(
        "{}//{}/{}",
        location.protocol(),
        location.host(),
        rel_url.to_string()
    );
    link.send_future(async move {
        let result = fetch_json_content(url).await;
        callback(result)
    });
}

pub fn refresh<C, F, M>(ctx: &Context<C>, callback: F)
where
    C: Component,
    F: Fn(anyhow::Result<i64>) -> M + 'static,
    M: Into<C::Message>,
{
    fetch(
        ctx,
        "api/refresh",
        move |resp: anyhow::Result<RefreshTime>| {
            callback(resp.map(|refresh_time| refresh_time.timestamp))
        },
    );
}

pub type LumiErrors = Vec<lumi::Error>;
pub fn get_errors<C, F, M>(ctx: &Context<C>, callback: F)
where
    C: Component,
    F: Fn(anyhow::Result<LumiErrors>) -> M + 'static,
    M: Into<C::Message>,
{
    fetch(ctx, "api/errors", callback);
}

pub type Trie = TrieTable<String>;
pub fn get_trie<C, F, M>(root: &str, options: &TrieOptions, ctx: &Context<C>, callback: F)
where
    C: Component,
    F: Fn(anyhow::Result<Trie>) -> M + 'static,
    M: Into<C::Message>,
{
    let query = serde_urlencoded::to_string(&options).unwrap();
    let rel_url = format!("api/trie/{}?{}", root, query);
    fetch(ctx, &rel_url, callback);
}

pub fn get_balances<C, F, M>(ctx: &Context<C>, callback: F)
where
    C: Component,
    F: Fn(anyhow::Result<HashMap<String, Vec<Position>>>) -> M + 'static,
    M: Into<C::Message>,
{
    fetch(ctx, "api/balances", callback);
}

pub type Journal = Vec<JournalItem<String, Rc<lumi::Transaction>>>;
pub fn get_account_journal<C, F, M>(
    account: &str,
    options: &FilterOptions,
    ctx: &Context<C>,
    callback: F,
) where
    C: Component,
    F: Fn(anyhow::Result<(Journal, usize)>) -> M + 'static,
    M: Into<C::Message>,
{
    let query = serde_urlencoded::to_string(&options).unwrap();
    let rel_url = if !account.is_empty() {
        format!("api/account/{}?{}", account, query)
    } else {
        format!("api/journal/?{}", query)
    };
    fetch(ctx, &rel_url, callback);
}

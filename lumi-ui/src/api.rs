
use std::fmt::Display;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum Error {
    JsError,
    ParseJson,
    Fetch,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error {


}

impl From<gloo_net::Error> for Error {
    fn from(value: gloo_net::Error) -> Self {
        match value {
            gloo_net::Error::JsError(_) => Error::JsError,
            gloo_net::Error::SerdeError(_) => Error::ParseJson,
            gloo_net::Error::GlooError(_) => Error::Fetch,
        }
    }
}

async fn fetch<T>(path: &str) -> Result<T, Error>
where T: for<'de> Deserialize<'de>
{
    let location = gloo_utils::window().location();
    let protocol = location.protocol().unwrap();
    let host = location.host().unwrap();
    let host = "127.0.0.1:8003";
    let url = format!("{protocol}//{host}/api/{path}");
    log::info!("fetching {url}");
    let r = gloo_net::http::Request::get(&url)
        .send()
        .await?
        .json::<T>()
        .await?;
    Ok(r)
}

pub async fn get_errors() -> Result<Vec<lumi::Error>, Error> {
    fetch("errors").await
}

use crate::api::{self, FetchState};
use anyhow::Error;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct Props {
    pub callback: Callback<i64>,
}
pub struct RefreshButton {
    fetch_state: FetchState<i64>,
}

pub enum Msg {
    Refresh,
    Success(i64),
    Failure(Error),
}

impl Component for RefreshButton {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            fetch_state: FetchState::NotStarted,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Success(timestamp) => {
                self.fetch_state = FetchState::Success(timestamp);
                ctx.props().callback.emit(timestamp);
            }
            Msg::Failure(err) => {
                self.fetch_state = FetchState::Failed(err);
            }
            Msg::Refresh => {
                self.fetch_state = FetchState::Fetching;
                api::refresh(ctx, |result| match result {
                    Ok(timestamp) => Msg::Success(timestamp),
                    Err(err) => Msg::Failure(err),
                })
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let onclick = ctx.link().callback(|_| Msg::Refresh);
        html! {
            <span id={"refresh"} {onclick}>{"Refresh"}</span>
        }
    }
}

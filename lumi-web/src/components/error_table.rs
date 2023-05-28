use crate::api::{self, FetchState, LumiErrors};
use anyhow::Error;
use lumi::ErrorLevel;
use yew::{context::ContextHandle, prelude::*};

pub enum Msg {
    GetErrors,
    GetErrorsSuccess(LumiErrors),
    GetErrorsFail(Error),
}

pub struct ErrorTable {
    fetch_state: FetchState<LumiErrors>,
    _handle: ContextHandle<i64>,
}

impl Component for ErrorTable {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &yew::Context<Self>) -> Self {
        let (_, handle) = ctx
            .link()
            .context::<i64>(ctx.link().callback(|_| Msg::GetErrors))
            .expect("context to be set");
        ctx.link().send_message(Msg::GetErrors);
        Self {
            fetch_state: FetchState::NotStarted,
            _handle: handle,
        }
    }

    fn update(&mut self, ctx: &yew::Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::GetErrorsSuccess(errors) => {
                self.fetch_state = FetchState::Success(errors);
                true
            }

            Msg::GetErrorsFail(err) => {
                self.fetch_state = FetchState::Failed(err);
                true
            }
            Msg::GetErrors => {
                self.fetch_state = FetchState::Fetching;
                api::get_errors(ctx, |result| match result {
                    Ok(error_list) => Msg::GetErrorsSuccess(error_list),
                    Err(err) => Msg::GetErrorsFail(err),
                });
                false
            }
        }
    }

    fn view(&self, _ctx: &yew::Context<Self>) -> yew::Html {
        match self.fetch_state {
            FetchState::Failed(ref reason) => html! {<p>{format!("failed {}", reason)}</p>},
            FetchState::Fetching => html! {<p>{"loading"}</p>},
            FetchState::NotStarted => html! {<p>{"not started"}</p>},
            FetchState::Success(ref errors) => {
                let error_list: Vec<_> = errors.iter().map(|error| {
                    let error_type = match error.level {
                        ErrorLevel::Error => html!{<span class="error">{"Error"}</span>},
                        ErrorLevel::Info => html!{<span class="info">{"Info"}</span>},
                        ErrorLevel::Warning => html!{<span class="warning">{"Warning"}</span>},
                    };
                    html!{
                        <>
                            <p class="desc">{error_type}{": "}{&error.msg}</p>
                            <p class="src">{&error.src.file}{":"}{error.src.start.line}{":"}{error.src.start.col}</p>
                        </>
                    }
                }).collect();
                html! {<>{error_list}</>}
            }
        }
    }
}

use anyhow::Error;

use yew::context::ContextHandle;
use yew::prelude::*;
use yew_router::components::Link;

use crate::api::{self, FetchState, Trie};
use crate::route::Route;

use lumi_server_defs::TrieOptions;
use std::rc::Rc;

#[derive(Properties, Clone, PartialEq, Eq)]
pub struct Props {
    pub root: &'static str,
    pub options: Rc<String>,
}

pub enum Msg {
    GetTrie,
    GetTrieSuccess(Trie),
    GetTrieError(Error),
}
pub struct TrieTable {
    fetch_state: FetchState<Trie>,
    options: TrieOptions,
    _handle: ContextHandle<i64>,
}

impl Component for TrieTable {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (_, handle) = ctx
            .link()
            .context::<i64>(ctx.link().callback(|_| Msg::GetTrie))
            .expect("context to be set");
        ctx.link().send_message(Msg::GetTrie);
        let options = serde_urlencoded::from_str(&ctx.props().options).unwrap_or_default();
        Self {
            fetch_state: FetchState::NotStarted,
            options,
            _handle: handle,
        }
    }

    fn changed(&mut self, ctx: &Context<Self>) -> bool {
        self.fetch_state = FetchState::NotStarted;
        ctx.link().send_message(Msg::GetTrie);
        true
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::GetTrieError(err) => {
                self.fetch_state = FetchState::Failed(err);
                true
            }
            Msg::GetTrieSuccess(trie) => {
                self.fetch_state = FetchState::Success(trie);
                true
            }
            Msg::GetTrie => {
                self.fetch_state = FetchState::Fetching;
                api::get_trie(
                    ctx.props().root,
                    &self.options,
                    ctx,
                    |result| match result {
                        Ok(trie) => Msg::GetTrieSuccess(trie),
                        Err(err) => Msg::GetTrieError(err),
                    },
                );
                false
            }
        }
    }

    fn view(&self, _ctx: &yew::Context<Self>) -> yew::Html {
        match self.fetch_state {
            FetchState::Failed(ref reason) => html! {<p>{format!("failed {}", reason)}</p>},
            FetchState::Fetching => html! {<p>{"loading"}</p>},
            FetchState::NotStarted => html! {<p>{"not started"}</p>},
            FetchState::Success(ref trie) => {
                let mut heads = vec![html! {<th/>}];
                for currency in trie.currencies.iter() {
                    heads.push(html! {<th class="mono right">{currency}</th>})
                }
                let mut stack: Vec<(&String, usize)> = Vec::new();
                let rows: Vec<_> = trie
                    .rows
                    .iter()
                    .map(|row| {
                        let td_class = format!("l{}", row.level);
                        while let Some((_, last_level)) = stack.last() {
                            if *last_level < row.level {
                                break;
                            } else {
                                stack.pop();
                            }
                        }
                        stack.push((&row.name, row.level));
                        let full_account = stack
                            .iter()
                            .map(|(seg, _)| seg.as_str())
                            .collect::<Vec<_>>()
                            .join(":");
                        type Anchor = Link<Route>;
                        let dest = Route::Account { name: full_account };
                        let mut cols = vec![html! {
                            <td class={td_class}>
                                <Anchor to={dest} classes={"account"}>
                                    {&row.name}
                                </Anchor>
                            </td>
                        }];
                        for number in &row.numbers {
                            cols.push(html! {<td class="mono right">{number}</td>});
                        }
                        html! {<tr>{cols}</tr>}
                    })
                    .collect();

                html! {
                    <div class="card inline-block">
                        <table class="trie">
                            <tr>
                                {heads}
                            </tr>
                            {rows}
                        </table>
                    </div>
                }
            }
        }
    }
}

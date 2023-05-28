use std::collections::HashMap;

use crate::api::{self, FetchState, Journal};
use crate::components::{EntrySelector, TxnCell};
use crate::route::Route;
use anyhow::Error;
use lumi_server_defs::{FilterOptions, DEFAULT_ENTRIES_PER_PAGE};
use rust_decimal::Decimal;
use yew::context::ContextHandle;

use yew::prelude::*;
use yew_router::components::Link;
use yew_router::history::{BrowserHistory, History, Location};

#[derive(Properties, Clone, PartialEq, Eq)]
pub struct Props {
    pub account: String,
    pub options: String,
}

struct State {
    options: FilterOptions,
    expand_postings: bool,
}
pub struct JournalTable {
    state: State,

    fetch_state: FetchState<(Journal, usize)>,
    _handle: ContextHandle<i64>,
}

pub enum Msg {
    GetJournal,
    GetJournalError(Error),
    GetJournalSuccess(Journal, usize),
    ExpandPostings,
}

fn change_to_str(changes: &HashMap<String, Decimal>) -> String {
    let descriptions: Vec<String> = changes
        .iter()
        .filter(|(_, n)| !n.is_zero())
        .map(|(c, n)| format!("{} {}", n, c))
        .collect();
    descriptions.join("\n")
}

impl Component for JournalTable {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_message(Msg::GetJournal);
        let (_, handle) = ctx
            .link()
            .context::<i64>(ctx.link().callback(|_| Msg::GetJournal))
            .expect("context to be set");

        let options = serde_urlencoded::from_str(&ctx.props().options).unwrap_or_default();
        Self {
            fetch_state: FetchState::NotStarted,
            state: State {
                options,
                expand_postings: false,
            },
            _handle: handle,
        }
    }

    fn changed(&mut self, ctx: &Context<Self>) -> bool {
        self.state.options = serde_urlencoded::from_str(&ctx.props().options).unwrap_or_default();
        ctx.link().send_message(Msg::GetJournal);
        true
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::GetJournalError(err) => {
                self.fetch_state = FetchState::Failed(err);
                true
            }
            Msg::GetJournalSuccess(journal, total) => {
                self.fetch_state = FetchState::Success((journal, total));
                true
            }
            Msg::GetJournal => {
                log::info!("get journal called");
                self.fetch_state = FetchState::Fetching;
                let props = ctx.props();
                api::get_account_journal(&props.account, &self.state.options, ctx, |result| {
                    match result {
                        Ok((journal, total)) => Msg::GetJournalSuccess(journal, total),
                        Err(err) => Msg::GetJournalError(err),
                    }
                });
                false
            }
            Msg::ExpandPostings => {
                log::info!("Msg::ExpandPostings");
                self.state.expand_postings = !self.state.expand_postings;
                true
            }
        }
    }

    fn view(&self, ctx: &yew::Context<Self>) -> yew::Html {
        match self.fetch_state {
            FetchState::Failed(ref reason) => html! {<p>{format!("failed {}", reason)}</p>},
            FetchState::Fetching => html! {<p>{"loading"}</p>},
            FetchState::NotStarted => html! {<p>{"not started"}</p>},
            FetchState::Success((ref journal, total)) => {
                log::info!("journal table view, success branch");
                log::info!("show_postings = {}", self.state.expand_postings);
                let mut rows = vec![];
                let props = ctx.props();
                if !props.account.is_empty() {
                    for (index, item) in journal.iter().enumerate() {
                        let change_str = change_to_str(&item.changes);
                        let balance_str = change_to_str(&item.balance);
                        rows.push(html!{
                        <TxnCell txn={item.txn.clone()} change_balance={(change_str, balance_str)} index={index} show_postings={self.state.expand_postings} />
                    });
                    }
                } else {
                    for (index, item) in journal.iter().enumerate() {
                        rows.push(html!{
                        <TxnCell txn={item.txn.clone()} index={index} show_postings={self.state.expand_postings}/>
                    });
                    }
                }
                type Anchor = Link<Route, FilterOptions>;
                let mut options_change_order = self.state.options.clone();
                let current_route: Route = BrowserHistory::new().location().route().unwrap();
                let order_indicator = if options_change_order.old_first == Some(true) {
                    options_change_order.old_first = None;
                    html! {
                        <Anchor to={current_route.clone()} query={options_change_order}><div class="arrow-up"></div></Anchor>
                    }
                } else {
                    options_change_order.old_first = Some(true);
                    html! {
                        <Anchor to={current_route.clone()} query={options_change_order}><div class="arrow-down"></div></Anchor>
                    }
                };
                let head = if !props.account.is_empty() {
                    html! {
                        <tr class="head">
                            <th class="left date">{"Date"}{order_indicator}</th>
                            <th class="center flag">{"Flag"}</th>
                            <th class="left">{"Description"}</th>
                            <th class="right amount">{"Position"}</th>
                            <th class="right cost">{"Cost"}</th>
                            <th class="right amount">{"Price"}</th>
                            <th class="right amount">{"Change"}</th>
                            <th class="right amount">{"Balance"}</th>
                        </tr>
                    }
                } else {
                    html! {
                        <tr class="head">
                            <th class="left date">{"Date"}{order_indicator}</th>
                            <th class="center flag">{"Flag"}</th>
                            <th class="left">{"Description"}</th>
                            <th class="right amount">{"Position"}</th>
                            <th class="right cost">{"Cost"}</th>
                            <th class="right amount">{"Price"}</th>
                        </tr>
                    }
                };
                log::info!("rows len = {}", rows.len());
                let table = if !rows.is_empty() {
                    html! {
                        <div class="card">
                            <table class="txn">
                                {head}
                                {rows}
                            </table>
                        </div>
                    }
                } else {
                    html! {
                        <div class="card">
                            <table class="txn">
                                {head}
                            </table>
                        </div>
                    }
                };

                let entries = self
                    .state
                    .options
                    .entries
                    .unwrap_or(DEFAULT_ENTRIES_PER_PAGE);
                let current_page = self.state.options.page.unwrap_or(1);
                let total_pages = (total + entries - 1) / entries;
                let mut link_pages = vec![];
                if current_page > 0 && current_page <= total_pages {
                    if current_page > 4 {
                        link_pages.extend(&[1, 0, current_page - 1]);
                    } else {
                        for p in 1..current_page {
                            link_pages.push(p);
                        }
                    }
                    link_pages.push(current_page);
                    if current_page + 3 < total_pages {
                        link_pages.extend(&[current_page + 1, 0, total_pages]);
                    } else {
                        for p in current_page + 1..=total_pages {
                            link_pages.push(p);
                        }
                    }
                } else if total_pages > 4 {
                    link_pages.extend(&[1, 2, 3, 0, total_pages]);
                } else {
                    link_pages = (1..=total_pages).into_iter().collect();
                }
                let mut page_buttons = vec![];
                if current_page > 1 {
                    let mut options_prev = self.state.options.clone();
                    match options_prev.page.as_mut() {
                        Some(n) if *n > 2 => *n -= 1,
                        _ => options_prev.page = None,
                    };
                    page_buttons.push(html!{
                    <Anchor to={current_route.clone()} query={options_prev} classes="button">{"<"}</Anchor>
                })
                }
                for p in link_pages {
                    let button = if p == 0 {
                        html! {
                            <a class="button">{"..."}</a>
                        }
                    } else if p == current_page {
                        html! {
                            <a class="button selected">{current_page}</a>
                        }
                    } else {
                        let mut option = self.state.options.clone();
                        let button_class: &str;
                        match p {
                            1 => {
                                option.page = None;
                                button_class = "button";
                            }
                            q if q == total_pages => {
                                option.page = Some(p);
                                button_class = "button";
                            }
                            _ => {
                                option.page = Some(p);
                                button_class = "button extra-page";
                            }
                        };
                        html! {
                            <Anchor to={current_route.clone()} query={option} classes={button_class}>{p}</Anchor>
                        }
                    };
                    page_buttons.push(button);
                }

                if current_page < total_pages {
                    let mut options_next = self.state.options.clone();
                    match options_next.page.as_mut() {
                        Some(n) => *n += 1,
                        _ => options_next.page = Some(2),
                    };
                    page_buttons.push(html!{
                    <Anchor to={current_route} query={options_next} classes="button">{">"}</Anchor>
                });
                }
                let current_entries = self
                    .state
                    .options
                    .entries
                    .unwrap_or(DEFAULT_ENTRIES_PER_PAGE);
                log::info!("current_entries={}", current_entries);
                let row_selector = html! {
                    <div class="row-selector">
                        <EntrySelector entries={current_entries}/>
                        <div class="buttons">
                            {page_buttons}
                        </div>
                    </div>
                };

                let onclick_expand = ctx.link().callback(|_| Msg::ExpandPostings);

                let class_expand = if self.state.expand_postings {
                    "button selected"
                } else {
                    "button"
                };
                html! {
                    <>
                        <div class="txn-table-head">
                            <span onclick={onclick_expand} class={class_expand}>{"Expand Positions"}</span>
                            {row_selector}
                        </div>
                        {table}
                    </>
                }
            }
        }
    }
}

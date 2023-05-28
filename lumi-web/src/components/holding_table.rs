use crate::api::{self, FetchState};
use crate::components::AccountRef;
use anyhow::Error;
use chrono::MIN_DATE;
use lumi_server_defs::Position;

use std::collections::HashMap;

use yew::context::ContextHandle;
use yew::prelude::*;
#[derive(Properties, Clone, PartialEq)]
pub struct Props {}

type HoldingMap = HashMap<String, Vec<Position>>;

pub struct HoldingTable {
    fetch_state: FetchState<HoldingMap>,
    _handle: ContextHandle<i64>,
}

pub enum Msg {
    GetHoldings,
    GetHoldingsSuccess(HoldingMap),
    GetHoldingsError(Error),
}

impl Component for HoldingTable {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (_, handle) = ctx
            .link()
            .context::<i64>(ctx.link().callback(|_| Msg::GetHoldings))
            .expect("context to be set");
        ctx.link().send_message(Msg::GetHoldings);
        Self {
            fetch_state: FetchState::NotStarted,
            _handle: handle,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::GetHoldingsError(err) => {
                self.fetch_state = FetchState::Failed(err);
                true
            }
            Msg::GetHoldingsSuccess(holdings) => {
                self.fetch_state = FetchState::Success(holdings);
                true
            }
            Msg::GetHoldings => {
                self.fetch_state = FetchState::Fetching;
                api::get_balances(ctx, |result| match result {
                    Ok(holdings) => Msg::GetHoldingsSuccess(holdings),
                    Err(err) => Msg::GetHoldingsError(err),
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
            FetchState::Success(ref holdings) => {
                let mut rows: Vec<Html> = vec![html! {
                    <tr>
                        <th class={"left"}>{"Account"}</th>
                        <th class={"right"}>{"Amount"}</th>
                        <th class={"right"}>{"Cost"}</th>
                        <th class={"right"}>{"Acquisition Date"}</th>
                        <th class={"right"}>{"Book Value"}</th>
                    </tr>
                }];
                let mut entries = holdings.iter().collect::<Vec<_>>();
                entries.sort_by_key(|t| t.0);
                for (account, account_map) in entries {
                    if !account.starts_with("Assets") && !account.starts_with("Lia") {
                        continue;
                    }
                    let mut account_entries = account_map.iter().collect::<Vec<_>>();
                    account_entries.sort_by_key(|p| {
                        (
                            &p.currency,
                            p.cost
                                .as_ref()
                                .map_or(MIN_DATE.naive_local(), |cost| cost.date),
                        )
                    });
                    for position in account_entries {
                        if position.number.is_zero() {
                            continue;
                        }
                        if let Some(cost) = &position.cost {
                            rows.push(html!{
                                <tr>
                                    <td class={"left"}><AccountRef account={account.clone()}/></td>
                                    <td class={"mono right"}>{position.number}{" "}{&position.currency}</td>
                                    <td class={"mono right"}>{&cost.amount}</td>
                                    <td class={"mono right"}>{&cost.date}</td>
                                    <td class={"mono right"}>{position.number*cost.amount.number}{" "}{&cost.amount.currency}</td>
                                </tr>
                            })
                        } else {
                            rows.push(html!{
                                <tr>
                                    <td class={"left"}><AccountRef account={account.clone()}/></td>
                                    <td class={"mono right"}>{position.number}{" "}{&position.currency}</td>
                                    <td class={"mono right"}></td>
                                    <td class={"mono right"}></td>
                                    <td class={"mono right"}>{position.number}{" "}{&position.currency}</td>
                                </tr>
                            })
                        }
                    }
                }
                html! {
                    <div class={"card"}>
                        <table class={"holdings"}>{rows}</table>
                    </div>
                }
            }
        }
    }
}

use lumi::{Transaction, TxnFlag};

use crate::components::AccountRef;
use std::rc::Rc;
use yew::prelude::*;

#[derive(Properties, Clone, PartialEq, Eq)]
pub struct Props {
    pub txn: Rc<Transaction>,
    #[prop_or(false)]
    pub show_postings: bool,
    #[prop_or_default]
    pub change_balance: Option<(String, String)>,
    pub index: usize,
}

pub enum Msg {
    ShowHidePostings,
}

pub struct TxnCell {
    show_postings: bool,
}

fn flag_str(flag: TxnFlag) -> &'static str {
    match flag {
        TxnFlag::Posted => "*",
        TxnFlag::Balance => "bal",
        TxnFlag::Pad => "pad",
        TxnFlag::Pending => "!",
    }
}

fn even_odd(index: usize) -> &'static str {
    if (index & 1) == 0 {
        "even"
    } else {
        "odd"
    }
}

fn desc(txn: &Transaction) -> Html {
    if !txn.payee().is_empty() {
        if !txn.narration().is_empty() {
            html! {
                <>
                    <strong>{txn.payee()}</strong>
                    {" "}
                    {txn.narration()}
                </>
            }
        } else {
            html! {
                <strong>{txn.payee()}</strong>
            }
        }
    } else {
        html! {
            {txn.narration()}
        }
    }
}

fn balance_view(props: &Props) -> Vec<Html> {
    props.txn.postings().iter().map(|posting| {
        let desc_span = if props.change_balance.is_some() {
            "5"
        } else {
            "1"
        };
        let tr_class = format!("balance {}", even_odd(props.index));
        let extra_td = if props.change_balance.is_some() {
            html! {}
        } else {
            html! {<td colspan={"2"}></td>}
        };
        html! {
            <tr class={tr_class}>
                <td class={"left mono date"}>{props.txn.date()}</td>
                <td class={"center mono flag"}>{"bal"}</td>
                <td class={"left"} colspan={desc_span}><AccountRef account={posting.account.to_string()} /></td>
                <td class={"right amount mono"}>{&posting.amount}</td>
                {extra_td}
            </tr>
        }
    }).collect::<Vec<Html>>()
}

fn posting_view(ctx: &Context<TxnCell>, show_postings: bool) -> Vec<Html> {
    let props = ctx.props();
    let mut result = Vec::new();
    let onclick = ctx.link().callback(|_| Msg::ShowHidePostings);

    let indicators = "•".repeat(props.txn.postings().len());
    let desc = html! {
        <>
            <td class={"left"}>
                {desc(&props.txn)}
            </td>
            <td class={"expand mono right"}>
                <span onclick={onclick}>{indicators}</span>
            </td>
        </>
    };

    let tr_class = format!("txn {}", even_odd(props.index));
    if let Some((change, balance)) = &props.change_balance {
        result.push(html! {
            <tr class={tr_class}>
                <td class={"left mono date"}>{props.txn.date()}</td>
                <td class={"center mono flag"}>{flag_str(props.txn.flag())}</td>
                {desc}
                <td colspan={"2"}></td>
                <td class={"right amount mono"}>{change}</td>
                <td class={"right amount mono"}>{balance}</td>
            </tr>
        })
    } else {
        result.push(html! {
            <tr class={tr_class}>
                <td class={"left mono date"}>{props.txn.date()}</td>
                <td class={"center mono flag"}>{flag_str(props.txn.flag())}</td>
                {desc}
                <td colspan={"2"}></td>
            </tr>
        })
    }
    let class_hide = if show_postings { "" } else { " hide" };
    let posting_class = format!("posting {}{}", even_odd(props.index), class_hide);
    for posting in props.txn.postings() {
        let price = posting
            .price
            .as_ref()
            .map(|p| p.to_string())
            .unwrap_or_default();
        let cost = posting
            .cost
            .as_ref()
            .map(|c| html! {<>{&c.amount}<br/>{c.date}</>})
            .unwrap_or_default();
        let extra_td = if props.change_balance.is_some() {
            html! {<td colspan={"2"}></td>}
        } else {
            html! {}
        };
        result.push(html! {
            <tr class={&posting_class}>
                <td></td>
                <td></td>
                <td class={"left"}><AccountRef account={posting.account.to_string()} /></td>
                <td class={"right mono amount"}>{&posting.amount}</td>
                <td class={"right mono cost"}>{cost}</td>
                <td class={"right mono amount"}>{price}</td>
                {extra_td}
            </tr>
        });
    }
    result
}

impl Component for TxnCell {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            show_postings: ctx.props().show_postings,
        }
    }

    fn changed(&mut self, ctx: &Context<Self>) -> bool {
        self.show_postings = ctx.props().show_postings;
        true
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::ShowHidePostings => {
                self.show_postings = !self.show_postings;
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        if ctx.props().txn.flag() == TxnFlag::Balance {
            html! {<> {balance_view(ctx.props())} </>}
        } else {
            html! {<> {posting_view(ctx, self.show_postings)} </>}
        }
    }
}

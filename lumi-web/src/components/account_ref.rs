use crate::route::Route;

use yew::prelude::*;
use yew_router::components::Link;

#[derive(Properties, Clone, PartialEq, Eq)]
pub struct Props {
    pub account: String,
}

#[function_component(AccountRef)]
pub fn account_ref(props: &Props) -> Html {
    type Anchor = Link<Route>;
    let dest = Route::Account {
        name: props.account.clone(),
    };
    html! {
        <Anchor to={dest} classes={"account"}>{&props.account}
        </Anchor>
    }
}

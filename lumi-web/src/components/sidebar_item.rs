use crate::route::Route;
use yew::prelude::*;
use yew_router::components::Link;
#[derive(Properties, Debug, PartialEq, Clone)]
pub struct Props {
    pub dest: Route,
    pub active: bool,
    pub title: &'static str,
}

#[function_component(SidebarItem)]
pub fn sidebar_item(props: &Props) -> Html {
    type Anchor = Link<Route>;
    if props.active {
        html! {
            <li class="active">
                <Anchor to={props.dest.clone()}>
                    <span>{&props.title}</span>
                </Anchor>
            </li>
        }
    } else {
        html! {
            <li>
                <Anchor to={props.dest.clone()}>
                    <span>{&props.title}</span>
                </Anchor>
            </li>
        }
    }
}

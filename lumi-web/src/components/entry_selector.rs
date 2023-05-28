use crate::route::Route;
use lumi_server_defs::{FilterOptions, DEFAULT_ENTRIES_PER_PAGE};
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Properties, Clone, Debug, PartialEq)]
pub struct Props {
    #[prop_or(DEFAULT_ENTRIES_PER_PAGE)]
    pub entries: usize,
}

#[function_component(EntrySelector)]
pub fn entry_selector(props: &Props) -> Html {
    let show_menu = use_state_eq(|| false);
    let show_menu_onclick = {
        let show_menu = show_menu.clone();
        Callback::from(move |_| show_menu.set(!*show_menu))
    };
    let location = use_location().unwrap();
    let query = location.search();
    let mut chars = query.chars();
    chars.next();
    let query = chars.as_str();
    let current_option: FilterOptions = serde_urlencoded::from_str(query).unwrap_or_default();
    let _current_path = location.pathname();

    let menu_items: Vec<_> = [20, 50, 100]
        .iter()
        .map(|n| {
            let n = *n;
            let mut new_option = current_option.clone();
            if n == DEFAULT_ENTRIES_PER_PAGE {
                new_option.entries = None;
            } else {
                new_option.entries = Some(n);
            }
            type Anchor = Link<Route, FilterOptions>;
            let item_class = if new_option.entries == current_option.entries {
                "entry-number button selected"
            } else {
                "entry-number button"
            };
            let route: Route = location.route().unwrap();
            html! {
                <Anchor to={route} query={new_option} classes={item_class}>{n}</Anchor>
            }
        })
        .collect();
    let menu_class = if *show_menu {
        "entry-menu"
    } else {
        "entry-menu hide"
    };
    let menu_button = if *show_menu {
        html! {
            <span onclick={show_menu_onclick} class="button selected">{props.entries}{" rows"}<div class="arrow-up"></div></span>
        }
    } else {
        html! {
            <span onclick={show_menu_onclick} class="button">{props.entries}{" rows"}<div class="arrow-down"></div></span>
        }
    };
    html! {
        <div class="select-entries">
            {menu_button}
            <div class={menu_class}>
                {menu_items}
            </div>
        </div>
    }
}

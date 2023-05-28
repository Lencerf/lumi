use crate::components::sidebar_item::SidebarItem;
use crate::route::Route;
use yew::{function_component, html, use_state_eq, Callback};
use yew_router::history::Location;
use yew_router::hooks::use_location;

#[function_component(Sidebar)]
pub fn sidebar() -> Html {
    let always_show = use_state_eq(|| false);
    let item_info = vec![
        (Route::Balance, "Balance Sheet"),
        (Route::Income, "Income Statement"),
        (Route::Journal, "Journal"),
        (Route::Holdings, "Holdings"),
        (Route::Errors, "Errors"),
    ];
    let location = use_location().unwrap();
    let current = location.route::<Route>();
    let items: Vec<_> = item_info
        .into_iter()
        .map(|(dest, title)| {
            html! {<SidebarItem dest={dest} active={current==Some(dest.clone())} title={title}/>}
        })
        .collect();
    let ul = html! {
        <ul>
            {items}
        </ul>
    };

    let class_name = if *always_show {
        "sidebar show"
    } else {
        "sidebar"
    };
    let hide_self = {
        let always_show = always_show.clone();
        Callback::from(move |_| always_show.set(false))
    };
    let show_self = {
        let always_show = always_show;
        Callback::from(move |_| always_show.set(true))
    };
    html! {
        <>
        <div id="show_sidebar"><span onclick={show_self} >{"☰"}</span></div>
        <div class={class_name}>
            <div class="title">
                <h1>{"Lumi"}</h1>
                <span id="hide_sidebar" onclick={&hide_self}>{"←"}</span>
            </div>
            <nav onclick={&hide_self}>
                {ul}
            </nav>
        </div>
        </>
    }
}

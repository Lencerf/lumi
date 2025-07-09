use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{components::*, path};

#[path = "components/components.rs"]
mod components;
mod pages;

use crate::components::sidebar::Sidebar;

use self::pages::{Account, BalanceSheet, Errors, Holdings, Income, Journal, NotFound};

#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();

    let redirect = || view! { <Redirect path="/balance_sheet" /> };
    let container = || {
        view! {
            <Sidebar />
            <div class="right-wrap">
                <Outlet />
            </div>
        }
    };
    view! {
        <Router>
            <Routes fallback=NotFound>
                <ParentRoute path=path!("/") view=container>
                    <Route path=path!("") view=redirect />
                    <Route path=path!("index.html") view=redirect />
                    <Route path=path!("balance_sheet") view=BalanceSheet />
                    <Route path=path!("income") view=Income />
                    <Route path=path!("holdings") view=Holdings />
                    <Route path=path!("journal") view=Journal />
                    <Route path=path!("errors") view=Errors />
                    <Route path=path!("account/:name") view=Account />
                    <Route path=path!("*any") view=NotFound />
                </ParentRoute>
            </Routes>
        </Router>
    }
}

fn main() {
    // set up logging
    _ = console_log::init_with_level(log::Level::Debug);
    console_error_panic_hook::set_once();

    mount_to_body(|| {
        view! { <App /> }
    })
}

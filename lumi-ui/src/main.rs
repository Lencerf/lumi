use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{components::*, path};

#[path = "components/components.rs"]
mod components;
#[path = "pages/pages.rs"]
mod pages;

use self::pages::home::Home;

#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();

    view! {
        <Router>
            <Routes fallback=|| view! { NotFound }>
                <Route path=path!("/") view=Home />
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

use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn Sidebar() -> impl IntoView {
    let items = [
        ("/balance_sheet", "Balance Sheet"),
        ("/income", "Income Statement"),
        ("/journal", "Journal"),
        ("/holdings", "Holdings"),
        ("/errors", "Errors"),
    ]
    .map(|(dst, text)| {
        view! {
            <li>
                <A href=dst>
                    <span>{text}</span>
                </A>
            </li>
        }
    });
    view! {
        <div class="sidebar show">
            <div class="title">
                <h1>"Lumi"</h1>
                <span id="hide_sidebar">"←"</span>
            </div>
            <nav>
                <ul>{items}</ul>
            </nav>
        </div>
    }
}

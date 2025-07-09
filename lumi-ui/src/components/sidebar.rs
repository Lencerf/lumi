use leptos::prelude::*;

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
                <a href=dst>
                    <span>{text}</span>
                </a>
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

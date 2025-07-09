use leptos::prelude::*;

#[component]
pub fn BalanceSheet() -> impl IntoView {
    view! {
        <header>
            <span id="title">"Balance Sheet"</span>
        </header>
        <main></main>
    }
}

#[component]
pub fn Income() -> impl IntoView {
    view! {
        <header>
            <span id="title">"Income Statement"</span>
        </header>
        <main></main>
    }
}

#[component]
pub fn Holdings() -> impl IntoView {
    view! {
        <header>
            <span id="title">"Holdings"</span>
        </header>
        <main></main>
    }
}

#[component]
pub fn Journal() -> impl IntoView {
    view! {
        <header>
            <span id="title">"Journal"</span>
        </header>
        <main></main>
    }
}

#[component]
pub fn Errors() -> impl IntoView {
    view! {
        <header>
            <span id="title">"Errors"</span>
        </header>
        <main></main>
    }
}

#[component]
pub fn Account() -> impl IntoView {
    view! { <div class="container">account</div> }
}

#[component]
pub fn NotFound() -> impl IntoView {
    view! {
        <header>
            <span id="title">"NotFound"</span>
        </header>
    }
}

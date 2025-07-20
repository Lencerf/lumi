use leptos::prelude::*;
use lumi::ErrorLevel;

use crate::api;

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
    let errors = LocalResource::new(move || api::get_errors());

    let error_cell = |e: &lumi::Error| {
        let error_type = match e.level {
            ErrorLevel::Error => view! { <span class="error">{"Error"}</span> },
            ErrorLevel::Info => view! { <span class="info">{"Info"}</span> },
            ErrorLevel::Warning => view! { <span class="warning">{"Warning"}</span> },
        };
        view! {
            <p class="desc">{error_type}{format!(": {}", e.msg)}</p>
            <p class="src">{format!("{}:{}:{}", e.src.file, e.src.start.line, e.src.start.col)}</p>
        }
    };
    // let a = move || {
    //     let errors = errors.read();
    //     errors.as_ref().map(|r| {
    //         r.as_ref()
    //             .map(|errs| errs.iter().map(error_cell).collect::<Vec<_>>())
    //     })
    // };
    let uls = move || {
        Suspend::new(async move {
            errors.await.map(|errors| {
                errors.iter().map(error_cell).collect::<Vec<_>>()
            })
        })
    };
    let fallback = move |errors: ArcRwSignal<Errors>| {
        let error_list = move || {
            errors.with(|errors| {
                errors
                    .iter()
                    .map(|(_, e)| view! { <li>{e.to_string()}</li> })
                    .collect::<Vec<_>>()
            })
        };

        view! {
            <div class="error">
                <h2>"Error"</h2>
                <ul>{error_list}</ul>
            </div>
        }
    };

    view! {
        <header>
            <span id="title">"Errors"</span>
        </header>
        <main>
            <Transition fallback=|| view! { <div>"Loading..."</div> }>
                <ErrorBoundary fallback>{uls}</ErrorBoundary>
            </Transition>
        </main>
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

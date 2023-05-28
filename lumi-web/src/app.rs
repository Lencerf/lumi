use crate::components::{
    ErrorTable, HoldingTable, JournalTable, RefreshButton, Sidebar, TrieTable,
};
use crate::route::Route;
use std::rc::Rc;
use yew::prelude::*;
use yew_router::prelude::*;

#[function_component(App)]
pub fn app() -> Html {
    html! {
        <BrowserRouter>
            <Sidebar />
            <Switch<Route> render={Switch::render(switch)} />
        </BrowserRouter>
    }
}

fn switch(routes: &Route) -> Html {
    let qs = BrowserHistory::new().location().search();
    let mut qs_chars = qs.chars();
    qs_chars.next();
    let qs: Rc<String> = Rc::new(String::from(qs_chars.as_str()));
    html! { <MainContent route={routes.clone()} query={qs} /> }
}

#[derive(Properties, PartialEq)]
struct MainContentProps {
    route: Route,
    query: Rc<String>,
}

#[function_component(MainContent)]
fn main_content(props: &MainContentProps) -> Html {
    let routes = &props.route;
    let title = match routes {
        Route::Balance | Route::Index => "Balance Sheet",
        Route::Holdings => "Holdings",
        Route::Journal => "Journal",
        Route::Income => "Income",
        Route::Account { name } => name.as_str(),
        Route::Errors => "Errors",
    };
    let timestamp = use_state_eq(|| 0i64);
    let update_timestamp = {
        let timestamp = timestamp.clone();
        Callback::from(move |val| {
            timestamp.set(val);
            log::info!("Ledger updated: {}", val);
        })
    };
    let title_bar = html! {
        <header>
            <span id="title">{title}</span>
            <RefreshButton callback={update_timestamp} />
        </header>
    };
    let qs = &props.query;
    let content = match routes {
        Route::Index => {
            html! {
                <Redirect<Route> to={Route::Balance}/>
            }
        }
        Route::Balance => {
            html! {
                <>
                    <div class="column">
                        <TrieTable root="Assets" options={qs}/>
                    </div>
                    <div class="column">
                        <TrieTable root="Liabilities" options={qs}/>
                        <TrieTable root="Equity" options={qs}/>
                    </div>
                </>
            }
        }
        Route::Income => {
            html! {
                <>
                    <div class="column">
                        <TrieTable root="Income" options={qs}/>
                    </div>
                    <div class="column">
                        <TrieTable root="Expenses" options={qs}/>
                    </div>
                </>
            }
        }
        Route::Journal => {
            html! {
                <JournalTable account={""} options={qs.to_string()}/>
            }
        }
        Route::Holdings => {
            html! {
                <HoldingTable />
            }
        }
        Route::Account { name } => {
            html! {
                <JournalTable account={name.to_string()} options={qs.to_string()}/>
            }
        }
        Route::Errors => {
            html! {
                <ErrorTable/>
            }
        }
    };
    html! {
        <div class="right-wrap">
            {title_bar}
            <main>
                <ContextProvider<i64> context={*timestamp} >
                    {content}
                </ContextProvider<i64>>
            </main>
        </div>
    }
}

use yew_router::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, Routable)]
pub enum Route {
    #[at("/holdings")]
    Holdings,
    #[at("/account/:name")]
    Account { name: String },
    #[at("/journal")]
    Journal,
    #[at("/income")]
    Income,
    #[at("/errors")]
    Errors,
    #[at("/balance_sheet")]
    Balance,
    #[at("/")]
    Index,
}

use gpui_kit::SharedString;
use gpui_kit::assets::IconName;

/// A destination in the application: a sidebar report, or the journal of
/// one account reached from a report.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    BalanceSheet,
    Income,
    Journal,
    Holdings,
    Errors,
    Editor,
    /// The journal of the named account and its sub-accounts.
    Account(SharedString),
}

/// What a report asks the application to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportEvent {
    /// Show the journal of the named account.
    OpenAccount(SharedString),
}

impl Page {
    /// The destinations listed in the sidebar, in order.
    pub fn sidebar() -> [Page; 6] {
        [
            Page::BalanceSheet,
            Page::Income,
            Page::Journal,
            Page::Holdings,
            Page::Errors,
            Page::Editor,
        ]
    }

    /// The destination's name, as shown in the sidebar and as the page title.
    pub fn title(&self) -> SharedString {
        match self {
            Page::BalanceSheet => "Balance sheet".into(),
            Page::Income => "Income statement".into(),
            Page::Journal => "Journal".into(),
            Page::Holdings => "Holdings".into(),
            Page::Errors => "Errors".into(),
            Page::Editor => "Editor".into(),
            Page::Account(account) => account.clone(),
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Page::BalanceSheet => IconName::Scale,
            Page::Income => IconName::TrendingUp,
            Page::Journal | Page::Account(_) => IconName::ScrollText,
            Page::Holdings => IconName::Wallet,
            Page::Errors => IconName::TriangleAlert,
            Page::Editor => IconName::SquarePen,
        }
    }

    /// The page named by `name`: a sidebar page's identifier such as
    /// `balance-sheet` or `journal`, or `account:` followed by an account
    /// name. `None` for anything else.
    pub fn parse(name: &str) -> Option<Page> {
        if let Some(account) = name.strip_prefix("account:") {
            return (!account.is_empty()).then(|| Page::Account(account.to_string().into()));
        }
        Page::sidebar()
            .into_iter()
            .find(|page| page.id().as_ref() == name)
    }

    /// The account whose journal this page shows, if it is an account page.
    pub fn account(&self) -> Option<&SharedString> {
        match self {
            Page::Account(account) => Some(account),
            _ => None,
        }
    }

    /// A stable identifier for element IDs.
    pub(crate) fn id(&self) -> SharedString {
        match self {
            Page::BalanceSheet => "balance-sheet".into(),
            Page::Income => "income".into(),
            Page::Journal => "journal".into(),
            Page::Holdings => "holdings".into(),
            Page::Errors => "errors".into(),
            Page::Editor => "editor".into(),
            Page::Account(account) => format!("account-{account}").into(),
        }
    }
}

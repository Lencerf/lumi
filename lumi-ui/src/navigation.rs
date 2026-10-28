use gpui_kit::assets::IconName;

/// A destination in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    BalanceSheet,
    Income,
}

impl Page {
    /// Every destination, in sidebar order.
    pub const ALL: [Page; 2] = [Page::BalanceSheet, Page::Income];

    /// The destination's name, as shown in the sidebar and as the page title.
    pub fn title(self) -> &'static str {
        match self {
            Page::BalanceSheet => "Balance sheet",
            Page::Income => "Income statement",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Page::BalanceSheet => IconName::Scale,
            Page::Income => IconName::TrendingUp,
        }
    }

    /// A stable identifier for element IDs.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Page::BalanceSheet => "balance-sheet",
            Page::Income => "income",
        }
    }
}

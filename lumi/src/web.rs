use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use crate::{Currency, UnitCost};
use chrono::Datelike;
pub use chrono::NaiveDate;
use rust_decimal::Decimal;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub currency: Currency,
    pub number: Decimal,
    pub cost: Option<UnitCost>,
}

pub const DEFAULT_ENTRIES_PER_PAGE: usize = 50;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(default))]
pub struct FilterOptions {
    pub entries: Option<usize>,
    pub page: Option<usize>,
    pub old_first: Option<bool>,
    pub account: Option<String>,
    pub time: Option<String>,
    pub query: Option<String>,
    pub flags: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(default))]
pub struct TrieOptions {
    pub show_closed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TrieNode<S: Eq + Hash> {
    pub numbers: HashMap<S, Decimal>,
    pub nodes: HashMap<S, TrieNode<S>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TrieTable<S> {
    pub rows: Vec<TrieTableRow<S>>,
    pub currencies: Vec<S>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TrieTableRow<S> {
    pub level: usize,
    pub name: S,
    pub numbers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct JournalItem<C: Hash + Eq, T> {
    pub txn: T,
    pub balance: HashMap<C, Decimal>,
    pub changes: HashMap<C, Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RefreshTime {
    pub timestamp: i64,
}

/// Vocabulary extracted from a [`Ledger`](crate::Ledger) for entry editing
/// auto-completion.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Completions {
    /// Open accounts in lexicographical order.
    pub accounts: Vec<String>,
    /// Distinct currencies across options, postings, account declarations, and
    /// commodity directives, with recently used currencies first.
    pub currencies: Vec<String>,
    /// Distinct non-empty payees, most recently used first.
    pub payees: Vec<String>,
    /// Distinct non-empty narrations, most recently used first.
    pub narrations: Vec<String>,
    /// Distinct non-empty narrations previously used with each payee, most
    /// recently used first.
    pub payee_narrations: HashMap<String, Vec<String>>,
    /// Distinct tags (including leading `#`), most recently used first.
    pub tags: Vec<String>,
    /// Distinct links (including leading `^`), most recently used first.
    pub links: Vec<String>,
    /// 1-based column at which posting decimal points should align, configured
    /// via `custom "lumi-option" "decimal-point-column"` (default `50`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub decimal_point_column: Option<usize>,
}

/// Aggregation granularity for periodic flow charts (such as the income
/// statement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum TimeInterval {
    Daily,
    #[default]
    Monthly,
    Quarterly,
    Yearly,
}

impl TimeInterval {
    /// All selectable intervals in chronological granularity order.
    pub const ALL: [Self; 4] = [Self::Daily, Self::Monthly, Self::Quarterly, Self::Yearly];

    /// Display label for interval selector buttons.
    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Monthly => "Monthly",
            Self::Quarterly => "Quarterly",
            Self::Yearly => "Yearly",
        }
    }

    /// Lowercase identifier for element IDs and query parameters.
    pub fn short_id(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::Yearly => "yearly",
        }
    }
}

/// Cumulative `Assets` and `Liabilities` balances over time, grouped by
/// currency.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BalanceSheetSeries {
    /// Sorted list of currencies that appear in `Assets` or `Liabilities`.
    pub currencies: Vec<Currency>,
    /// Chronological end-of-day snapshots on dates touching `Assets` or
    /// `Liabilities`.
    pub points: Vec<BalanceSheetPoint>,
}

/// One date's cumulative `Assets` and `Liabilities` balances per currency
/// (aligned by index with [`BalanceSheetSeries::currencies`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BalanceSheetPoint {
    pub date: NaiveDate,
    /// Cumulative `Assets` balance for each currency in `currencies`.
    pub assets: Vec<Decimal>,
    /// Cumulative `Liabilities` balance for each currency in `currencies`.
    pub liabilities: Vec<Decimal>,
}

impl BalanceSheetPoint {
    /// Proleptic Gregorian day number (`num_days_from_ce`) for proportional
    /// time-axis scaling.
    pub fn day_index(&self) -> i32 {
        self.date.num_days_from_ce()
    }
}

/// Cumulative balance of one account (including its sub-accounts) over time,
/// grouped by currency.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AccountBalanceSeries {
    /// Sorted list of currencies that appear in the account's postings.
    pub currencies: Vec<Currency>,
    /// Chronological end-of-day snapshots on dates touching the account.
    pub points: Vec<AccountBalancePoint>,
}

/// One date's cumulative account balance per currency (aligned by index with
/// [`AccountBalanceSeries::currencies`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AccountBalancePoint {
    pub date: NaiveDate,
    /// Cumulative balance for each currency in `currencies`.
    pub balances: Vec<Decimal>,
}

impl AccountBalancePoint {
    /// Proleptic Gregorian day number (`num_days_from_ce`) for proportional
    /// time-axis scaling.
    pub fn day_index(&self) -> i32 {
        self.date.num_days_from_ce()
    }
}

/// Periodic `Income` and `Expenses` totals grouped by [`TimeInterval`] and
/// currency.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IncomeExpenseSeries {
    pub interval: TimeInterval,
    /// Sorted list of currencies that appear in `Income` or `Expenses`.
    pub currencies: Vec<Currency>,
    /// Contiguous calendar buckets from the first to the last `Income` or
    /// `Expenses` posting.
    pub buckets: Vec<IncomeExpenseBucket>,
}

/// One calendar bucket's `Income` and `Expenses` totals per currency (aligned
/// by index with [`IncomeExpenseSeries::currencies`]).
///
/// `income` is negated from Beancount's credit convention (`-sum(Income)`) so
/// earned income is positive alongside positive `expenses` (`sum(Expenses)`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IncomeExpenseBucket {
    /// Display label (`YYYY-MM-DD`, `YYYY-MM`, `YYYY-Q1`..`Q4`, or `YYYY`).
    pub label: String,
    /// Earned income (`-sum(Income postings)`) per currency.
    pub income: Vec<Decimal>,
    /// Expenses (`sum(Expenses postings)`) per currency.
    pub expenses: Vec<Decimal>,
}

/// Formats a proleptic Gregorian day number (`num_days_from_ce`) as
/// `YYYY-MM-DD`.
pub fn format_day_index(day: i32) -> Option<String> {
    NaiveDate::from_num_days_from_ce_opt(day).map(|date| date.format("%Y-%m-%d").to_string())
}

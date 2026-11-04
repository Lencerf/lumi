use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use crate::{Currency, UnitCost};
pub use chrono::NaiveDate;
use chrono::{Datelike, Local, Months};
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

/// Inclusive `[start, end]` calendar date range for filtering chart series.
///
/// Either bound may be `None` to leave that end open (`All` has both `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ChartDateRange {
    pub start: Option<NaiveDate>,
    pub end: Option<NaiveDate>,
}

impl ChartDateRange {
    /// Unbounded date range (`All`).
    pub const ALL: Self = Self {
        start: None,
        end: None,
    };

    /// Creates an inclusive `[start, end]` range, ordering the two dates if
    /// `a > b`.
    pub fn between(a: NaiveDate, b: NaiveDate) -> Self {
        if a <= b {
            Self {
                start: Some(a),
                end: Some(b),
            }
        } else {
            Self {
                start: Some(b),
                end: Some(a),
            }
        }
    }

    pub fn is_unbounded(&self) -> bool {
        self.start.is_none() && self.end.is_none()
    }

    pub fn contains(&self, date: NaiveDate) -> bool {
        if let (Some(start), Some(end)) = (self.start, self.end)
            && start > end
        {
            return false;
        }
        self.start.is_none_or(|start| date >= start) && self.end.is_none_or(|end| date <= end)
    }
}

/// Quick-select date range presets for charts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum ChartDatePreset {
    OneYear,
    ThreeYears,
    TenYears,
    YearToDate,
    #[default]
    All,
    Custom,
}

impl ChartDatePreset {
    /// All selectable presets in toolbar display order.
    pub const ALL: [Self; 6] = [
        Self::OneYear,
        Self::ThreeYears,
        Self::TenYears,
        Self::YearToDate,
        Self::All,
        Self::Custom,
    ];

    /// Compact button label.
    pub fn label(self) -> &'static str {
        match self {
            Self::OneYear => "1Y",
            Self::ThreeYears => "3Y",
            Self::TenYears => "10Y",
            Self::YearToDate => "YTD",
            Self::All => "All",
            Self::Custom => "Custom",
        }
    }

    /// Descriptive tooltip label.
    pub fn tooltip(self) -> &'static str {
        match self {
            Self::OneYear => "Past 1 year",
            Self::ThreeYears => "Past 3 years",
            Self::TenYears => "Past 10 years",
            Self::YearToDate => "Year to date",
            Self::All => "All dates",
            Self::Custom => "Custom date range",
        }
    }

    /// Lowercase identifier for element IDs.
    pub fn short_id(self) -> &'static str {
        match self {
            Self::OneYear => "1y",
            Self::ThreeYears => "3y",
            Self::TenYears => "10y",
            Self::YearToDate => "ytd",
            Self::All => "all",
            Self::Custom => "custom",
        }
    }

    /// Resolves a fixed preset relative to `today`, returning `None` for
    /// [`Self::Custom`] (whose range comes from user selection).
    pub fn resolve(self, today: NaiveDate) -> Option<ChartDateRange> {
        match self {
            Self::OneYear => Some(ChartDateRange {
                start: today.checked_sub_months(Months::new(12)),
                end: Some(today),
            }),
            Self::ThreeYears => Some(ChartDateRange {
                start: today.checked_sub_months(Months::new(36)),
                end: Some(today),
            }),
            Self::TenYears => Some(ChartDateRange {
                start: today.checked_sub_months(Months::new(120)),
                end: Some(today),
            }),
            Self::YearToDate => Some(ChartDateRange {
                start: NaiveDate::from_ymd_opt(today.year(), 1, 1),
                end: Some(today),
            }),
            Self::All => Some(ChartDateRange::ALL),
            Self::Custom => None,
        }
    }
}

/// Today's local calendar date.
pub fn today_date() -> NaiveDate {
    Local::now().date_naive()
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

impl BalanceSheetSeries {
    /// Earliest and latest snapshot dates in `points`, if non-empty.
    pub fn date_bounds(&self) -> Option<(NaiveDate, NaiveDate)> {
        Some((self.points.first()?.date, self.points.last()?.date))
    }

    /// Slices this cumulative series to `range`, carrying forward any balance
    /// accrued strictly before `range.start` onto `range.start` and extending
    /// the final balance horizontally to `range.end` when bounded.
    pub fn slice(&self, range: ChartDateRange) -> Self {
        if range.is_unbounded() || self.points.is_empty() {
            return self.clone();
        }
        if let (Some(start), Some(end)) = (range.start, range.end)
            && start > end
        {
            return Self {
                currencies: self.currencies.clone(),
                points: Vec::new(),
            };
        }

        let prior = range
            .start
            .and_then(|start| self.points.iter().rfind(|pt| pt.date < start));
        let in_range: Vec<&BalanceSheetPoint> = self
            .points
            .iter()
            .filter(|pt| range.contains(pt.date))
            .collect();

        let mut points = Vec::new();
        if let (Some(start), Some(prev)) = (range.start, prior)
            && in_range.first().is_none_or(|first| first.date > start)
        {
            points.push(BalanceSheetPoint {
                date: start,
                assets: prev.assets.clone(),
                liabilities: prev.liabilities.clone(),
            });
        }
        points.extend(in_range.into_iter().cloned());

        if let (Some(end), Some(last)) = (range.end, points.last().cloned())
            && last.date < end
        {
            points.push(BalanceSheetPoint {
                date: end,
                assets: last.assets,
                liabilities: last.liabilities,
            });
        }

        Self {
            currencies: self.currencies.clone(),
            points,
        }
    }
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

impl AccountBalanceSeries {
    /// Earliest and latest snapshot dates in `points`, if non-empty.
    pub fn date_bounds(&self) -> Option<(NaiveDate, NaiveDate)> {
        Some((self.points.first()?.date, self.points.last()?.date))
    }

    /// Slices this cumulative series to `range`, carrying forward any balance
    /// accrued strictly before `range.start` onto `range.start` and extending
    /// the final balance horizontally to `range.end` when bounded.
    pub fn slice(&self, range: ChartDateRange) -> Self {
        if range.is_unbounded() || self.points.is_empty() {
            return self.clone();
        }
        if let (Some(start), Some(end)) = (range.start, range.end)
            && start > end
        {
            return Self {
                currencies: self.currencies.clone(),
                points: Vec::new(),
            };
        }

        let prior = range
            .start
            .and_then(|start| self.points.iter().rfind(|pt| pt.date < start));
        let in_range: Vec<&AccountBalancePoint> = self
            .points
            .iter()
            .filter(|pt| range.contains(pt.date))
            .collect();

        let mut points = Vec::new();
        if let (Some(start), Some(prev)) = (range.start, prior)
            && in_range.first().is_none_or(|first| first.date > start)
        {
            points.push(AccountBalancePoint {
                date: start,
                balances: prev.balances.clone(),
            });
        }
        points.extend(in_range.into_iter().cloned());

        if let (Some(end), Some(last)) = (range.end, points.last().cloned())
            && last.date < end
        {
            points.push(AccountBalancePoint {
                date: end,
                balances: last.balances,
            });
        }

        Self {
            currencies: self.currencies.clone(),
            points,
        }
    }
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
    /// Sorted list of currencies that appear in `Income` across the ledger.
    #[cfg_attr(feature = "serde", serde(default))]
    pub income_currencies: Vec<Currency>,
    /// Sorted list of currencies that appear in `Expenses` across the ledger.
    #[cfg_attr(feature = "serde", serde(default))]
    pub expense_currencies: Vec<Currency>,
    /// Earliest `Income` or `Expenses` posting date in the ledger.
    #[cfg_attr(feature = "serde", serde(default))]
    pub first_date: Option<NaiveDate>,
    /// Latest `Income` or `Expenses` posting date in the ledger.
    #[cfg_attr(feature = "serde", serde(default))]
    pub last_date: Option<NaiveDate>,
    /// Contiguous calendar buckets from the first to the last matching `Income`
    /// or `Expenses` posting.
    pub buckets: Vec<IncomeExpenseBucket>,
}

impl IncomeExpenseSeries {
    /// Earliest and latest `Income` or `Expenses` posting dates across the
    /// ledger, if present.
    pub fn date_bounds(&self) -> Option<(NaiveDate, NaiveDate)> {
        Some((self.first_date?, self.last_date?))
    }
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

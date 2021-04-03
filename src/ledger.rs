use crate::parse::Parser;
pub use chrono::NaiveDate as Date;
pub use rust_decimal::Decimal;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::From;
use std::fmt;
use std::ops::{Div, Mul};
use std::sync::Arc;

/// Representing a location, line number and column number, in a source file.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub col: usize,
}

impl Location {
    pub fn advance(&self, width: usize) -> Self {
        Location {
            col: self.col + width,
            line: self.line,
        }
    }
}

impl From<(usize, usize)> for Location {
    fn from(tuple: (usize, usize)) -> Self {
        Location {
            line: tuple.0,
            col: tuple.1,
        }
    }
}

/// A string wrapped in [`Arc`](std::sync::Arc)
/// representing the source file path.
pub type SrcFile = Arc<String>;

/// Represents a range in a source file. This struct is used to track the origins
/// of any information in the generated [`Ledger`], as well as for locating errors.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Source {
    pub file: SrcFile,
    pub start: Location,
    pub end: Location,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.start.line, self.start.col)
    }
}

/// Kinds of errors that `lumi` encountered during generating [`Ledger`] from
/// files input text.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorType {
    /// IO error, e.g., the context of an input file cannot be read.
    Io,
    /// Syntax error in the source file.
    Syntax,
    /// Indicates a transactions is not balanced.
    NotBalanced,
    /// A transaction missing too much information such that `lumi` cannot infer
    /// for the context.
    Incomplete,
    /// An unopened or already closed account is referred.
    Account,
    /// `lumi` cannot find a position in the running balance sheet that matching
    /// the cost basis provided in the posting.
    NoMatch,
    /// Multiple Positions are founded in the running balance sheet that matching
    /// the cost basis provided in the posting.
    Ambiguous,
    /// Duplicate information, such as two identical tags in a single transaction.
    Duplicate,
}

/// The level of an error. Any information in the source file resulting an
/// [`ErrorLevel::Error`] are dropped.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorLevel {
    Info,
    Warning,
    Error,
}
/// Contains the full information of an error.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Error {
    pub msg: String,
    pub src: Source,
    pub r#type: ErrorType,
    pub level: ErrorLevel,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}: {}\n  {}:{}:{}",
            self.level, self.msg, self.src.file, self.src.start.line, self.src.start.col
        )
    }
}

pub type Currency = String;

/// A [`Decimal`] number plus the currency.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Amount {
    pub number: Decimal,
    pub currency: Currency,
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.number, self.currency)
    }
}

impl<'a> Div<Decimal> for &'a Amount {
    type Output = Amount;

    fn div(self, rhs: Decimal) -> Self::Output {
        Amount {
            number: self.number / rhs,
            currency: self.currency.clone(),
        }
    }
}

impl<'a> Mul<Decimal> for &'a Amount {
    type Output = Amount;

    fn mul(self, rhs: Decimal) -> Self::Output {
        Amount {
            number: self.number * rhs,
            currency: self.currency.clone(),
        }
    }
}

/// The unit price (`@`) or total price (`@@`) of the amount in a posting.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub enum Price {
    Unit(Amount),
    Total(Amount),
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Price::Unit(amount) => write!(f, "@ {}", amount),
            Price::Total(amount) => write!(f, "@@ {}", amount),
        }
    }
}

/// The cost basis information (unit cost and transaction date) used to identify
/// a position in the running balances.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct UnitCost {
    /// The unit cost basis.
    pub amount: Amount,
    /// The transaction date.
    pub date: Date,
}

impl fmt::Display for UnitCost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{ {}, {} }}", self.amount, self.date)
    }
}

/// The flag of a [`Transaction`].
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TxnFlag {
    /// transactions flagged by `?`.
    Pending,
    /// transactions flagged by `txn` or `*`.
    Posted,
    /// `pad` directives.
    Pad,
    /// `balance` directives.
    Balance,
}

impl fmt::Display for TxnFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxnFlag::Pending => write!(f, "!"),
            TxnFlag::Posted | TxnFlag::Pad => write!(f, "*"),
            TxnFlag::Balance => write!(f, "balance"),
        }
    }
}

/// A string wrapped in [`Arc`](std::sync::Arc)
/// representing the account name.
pub type Account = Arc<String>;

/// A posting like `Assets::Bank -100 JPY` inside a [`Transaction`].
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub struct Posting {
    pub account: Account,
    pub amount: Amount,
    pub cost: Option<UnitCost>,
    pub price: Option<Price>,
    pub meta: Meta,
    pub src: Source,
}

impl fmt::Display for Posting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let num_str = self.amount.to_string();
        let index = num_str.find(|c| c == ' ' || c == '.').unwrap();
        let width = f.width().unwrap_or(46) - 1;
        let account_width = std::cmp::max(self.account.len() + 1, width - index);
        write!(
            f,
            "{:width$}{}",
            self.account,
            num_str,
            width = account_width
        )?;
        if let Some(cost) = &self.cost {
            write!(f, " {}", cost)?;
        }
        if let Some(ref price) = self.price {
            write!(f, " {}", price)?;
        }
        Ok(())
    }
}

/// Represents a transaction, or a `pad` directives, or a `balance` directive in
/// the source file.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub struct Transaction {
    pub date: Date,
    pub flag: TxnFlag,
    pub payee: String,
    pub narration: String,
    pub links: Vec<String>,
    pub tags: Vec<String>,
    pub meta: Meta,
    pub postings: Vec<Posting>,
    pub src: Source,
}

/// Represents a `note` directive
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub struct AccountNote {
    pub date: Date,
    pub val: String,
    pub src: Source,
}

/// Represents a `document` directive
pub type AccountDoc = AccountNote;

/// Represents the meta data attached to a commodity, a transaction, or a posting.
pub type Meta = HashMap<String, (String, Source)>;

/// Contains the open/close date of an account, as well as the notes and documents.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub struct AccountInfo {
    pub open: (Date, Source),
    pub close: Option<(Date, Source)>,
    pub currencies: HashSet<Currency>,
    pub notes: Vec<AccountNote>,
    pub docs: Vec<AccountDoc>,
    pub meta: Meta,
}

/// Represents an `event` directive.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub struct EventInfo {
    pub date: Date,
    pub desc: String,
    pub src: Source,
}

impl From<(Date, String, Source)> for EventInfo {
    fn from(tuple: (Date, String, Source)) -> Self {
        EventInfo {
            date: tuple.0,
            desc: tuple.1,
            src: tuple.2,
        }
    }
}

/// Represents the final balances of all accounts.
pub type BalanceSheet = HashMap<Account, HashMap<Currency, HashMap<Option<UnitCost>, Decimal>>>;

/// Represents a valid ledger containing all valid accounts and balanced
/// transactions.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug)]
pub struct Ledger {
    pub accounts: HashMap<Account, AccountInfo>,
    pub commodities: HashMap<Currency, (Meta, Source)>,
    pub txns: Vec<Transaction>,
    pub options: HashMap<String, (String, Source)>,
    pub events: HashMap<String, Vec<EventInfo>>,
    pub(crate) balance_sheet: BalanceSheet,
}

impl Ledger {
    pub fn balance_sheet(&self) -> &BalanceSheet {
        &self.balance_sheet
    }

    pub fn from_file(path: &str) -> (Self, Vec<Error>) {
        let (draft, mut errors) = Parser::parse(path);
        let ledger = draft.into_ledger(&mut errors);
        (ledger, errors)
    }
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.flag {
            TxnFlag::Balance => write!(f, "{} {}", self.date, self.flag)?,
            _ => write!(
                f,
                "{} {} \"{}\" \"{}\"",
                self.date, self.flag, self.payee, self.narration
            )?,
        };
        for tag in &self.tags {
            write!(f, " {}", tag)?;
        }
        for link in &self.links {
            write!(f, " {}", link)?;
        }
        for (key, val) in self.meta.iter() {
            write!(f, "\n  {}: {}", key, val.0)?;
        }
        let width = f.width().unwrap_or(50);
        match self.flag {
            TxnFlag::Balance => {
                if self.postings.len() == 1 {
                    write!(f, " {:width$}", self.postings[0], width = width - 19)?;
                } else {
                    for posting in self.postings.iter() {
                        write!(f, "\n    {:width$}", posting, width = width - 4)?;
                    }
                }
            }
            _ => {
                for posting in self.postings.iter() {
                    write!(f, "\n    {:width$}", posting, width = width - 4)?;
                }
            }
        }
        Ok(())
    }
}

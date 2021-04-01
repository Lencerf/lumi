pub use chrono::NaiveDate as Date;
pub use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::convert::From;
use std::fmt;
use std::ops::{Div, Mul};
use std::sync::Arc;

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

pub type SrcFile = Arc<String>;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorType {
    Io,
    Syntax,
    NotBalanced,
    Incomplete,
    Account,
    NoMatch,
    Ambiguous,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorLevel {
    Info,
    Warning,
    Error,
}

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

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct UnitCost {
    pub amount: Amount,
    pub date: Date,
}

impl fmt::Display for UnitCost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{ {}, {} }}", self.amount, self.date)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TxnFlag {
    Pending,
    Posted,
    Pad,
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

pub type Account = Arc<String>;

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

#[derive(Debug)]
pub struct AccountNote {
    pub date: Date,
    pub val: String,
    pub src: Source,
}

pub type AccountDoc = AccountNote;

pub type Meta = HashMap<String, (String, Source)>;

#[derive(Debug)]
pub struct AccountInfo {
    pub open: (Date, Source),
    pub close: Option<(Date, Source)>,
    pub currencies: HashSet<Currency>,
    pub notes: Vec<AccountNote>,
    pub docs: Vec<AccountDoc>,
    pub meta: Meta,
}

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

pub type BalanceSheet = HashMap<Account, HashMap<Currency, HashMap<Option<UnitCost>, Decimal>>>;

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

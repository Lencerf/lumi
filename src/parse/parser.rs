use crate::{
    Account, AccountDoc, AccountNote, Amount, Date, Decimal, EventInfo, Meta, Price, Source,
    TxnFlag, UnitCost,
};

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostBasis {
    Total(Amount),
    Unit(Amount),
}

impl CostBasis {
    pub fn to_unit_cost(&self, p_number: Decimal) -> Amount {
        match self {
            CostBasis::Total(amount) => amount / p_number.abs(),
            CostBasis::Unit(amount) => amount.clone(),
        }
    }

    pub fn currency(&self) -> &str {
        match self {
            CostBasis::Total(amount) => &amount.currency,
            CostBasis::Unit(amount) => &amount.currency,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CostLiteral {
    pub date: Option<Date>,
    pub basis: Option<CostBasis>,
}

impl CostLiteral {
    pub fn unwrap_unit_cost(self, p_number: Decimal) -> UnitCost {
        let date = self.date.unwrap();
        let amount = self.basis.unwrap().to_unit_cost(p_number);
        UnitCost {
            amount: amount,
            date,
        }
    }
}

#[derive(Debug)]
pub struct PostingDraft {
    pub account: Account,
    pub amount: Option<Amount>,
    pub cost: Option<CostLiteral>,
    pub price: Option<Price>,
    pub meta: Meta,
    pub src: Source,
}

#[derive(Debug)]
pub struct TxnDraft {
    pub date: Date,
    pub flag: TxnFlag,
    pub payee: String,
    pub narration: String,
    pub links: Vec<String>,
    pub tags: Vec<String>,
    pub meta: Meta,
    pub postings: Vec<PostingDraft>,
    pub src: Source,
}

#[derive(Debug, Default)]
pub struct AccountInfoDraft {
    pub open: Option<(Date, Source)>,
    pub close: Option<(Date, Source)>,
    pub currencies: HashSet<String>,
    pub notes: Vec<AccountNote>,
    pub docs: Vec<AccountDoc>,
    pub meta: Meta,
}

#[derive(Debug, Default)]
pub struct LedgerDraft {
    pub accounts: HashMap<Account, AccountInfoDraft>,
    pub commodities: HashMap<String, (Meta, Source)>,
    pub txns: Vec<TxnDraft>,
    pub options: HashMap<String, (String, Source)>,
    pub events: HashMap<String, Vec<EventInfo>>,
}

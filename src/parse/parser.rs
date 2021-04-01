use super::lexer::Lexer;
use super::token::Token;
use crate::{
    Account, AccountDoc, AccountNote, Amount, Currency, Date, Decimal, Error, ErrorLevel,
    ErrorType, EventInfo, Location, Meta, Price, Source, SrcFile, TxnFlag, UnitCost,
};

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt, fs,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};

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

impl fmt::Display for CostLiteral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut date_str = self.date.map_or("".to_string(), |date| date.to_string());
        if let Some(cost_basis) = &self.basis {
            if self.date.is_some() {
                date_str = format!(", {}", date_str);
            }
            match cost_basis {
                CostBasis::Total(total_amount) => {
                    write!(f, "{{{{ {}{} }}}}", total_amount, date_str)
                }
                CostBasis::Unit(unit_amount) => write!(f, "{{ {}{} }}", unit_amount, date_str),
            }
        } else {
            write!(f, "{{ {} }}", date_str)
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

impl AccountInfoDraft {
    pub fn merge(&mut self, another: AccountInfoDraft, name: &str) -> Vec<Error> {
        let AccountInfoDraft {
            open,
            close,
            currencies,
            notes,
            docs,
            meta,
        } = another;
        let mut errors = vec![];
        if let Some((_, src)) = &open {
            if let Some((_, existing_src)) = &self.open {
                errors.push(Error {
                    level: ErrorLevel::Error,
                    r#type: ErrorType::Duplicate,
                    msg: format!("Account {} has been opened at {}.", name, existing_src),
                    src: src.clone(),
                });
            }
        }
        if let Some((_, src)) = &close {
            if let Some((_, existing_src)) = &self.close {
                errors.push(Error {
                    level: ErrorLevel::Error,
                    r#type: ErrorType::Duplicate,
                    msg: format!("Account {} has been closed at {}.", name, existing_src),
                    src: src.clone(),
                });
            }
        }
        if errors.len() == 0 {
            if open.is_some() {
                self.open = open;
                self.currencies = currencies;
            }
            if close.is_some() {
                self.close = close;
            }
            self.meta.extend(meta);
            self.notes.extend(notes);
            self.docs.extend(docs);
        }
        errors
    }
}

#[derive(Debug, Default)]
pub struct LedgerDraft {
    pub accounts: HashMap<Account, AccountInfoDraft>,
    pub commodities: HashMap<String, (Meta, Source)>,
    pub txns: Vec<TxnDraft>,
    pub options: HashMap<String, (String, Source)>,
    pub events: HashMap<String, Vec<EventInfo>>,
}

impl LedgerDraft {
    pub fn add_option(&mut self, key: String, val: String, src: Source) -> Result<(), Error> {
        if let Some((_, existing_src)) = self.options.get(&key) {
            Err(Error {
                level: ErrorLevel::Warning,
                r#type: ErrorType::Duplicate,
                msg: format!(
                    "Ignored directive: option {} has been specified at {}.",
                    &key, existing_src
                ),
                src,
            })
        } else {
            self.options.insert(key, (val, src));
            Ok(())
        }
    }

    pub fn add_commodity(
        &mut self,
        commodity: String,
        meta: Meta,
        src: Source,
    ) -> Result<(), Error> {
        if let Some((_, existing_src)) = self.commodities.get(&commodity) {
            Err(Error {
                level: ErrorLevel::Warning,
                r#type: ErrorType::Duplicate,
                msg: format!(
                    "Ignored directive: commodity {} has been defined at {}.",
                    &commodity, existing_src
                ),
                src,
            })
        } else {
            self.commodities.insert(commodity, (meta, src));
            Ok(())
        }
    }
}

impl LedgerDraft {
    pub fn merge(&mut self, another: LedgerDraft) -> Vec<Error> {
        let mut errors = vec![];
        let LedgerDraft {
            accounts,
            commodities,
            txns,
            options,
            events,
        } = another;
        self.txns.extend(txns);
        for (name, list) in events {
            if let Some(l) = self.events.get_mut(&name) {
                l.extend(list);
            } else {
                self.events.insert(name, list);
            }
        }
        for (key, (val, src)) in options {
            if let Err(e) = self.add_option(key, val, src) {
                errors.push(e);
            }
        }
        for (currency, (meta, src)) in commodities {
            if let Err(e) = self.add_commodity(currency, meta, src) {
                errors.push(e);
            }
        }
        for (name, info) in accounts {
            if let Some(existing_info) = self.accounts.get_mut(&name) {
                let merge_errors = existing_info.merge(info, &name);
                errors.extend(merge_errors);
            } else {
                self.accounts.insert(name, info);
            }
        }
        errors
    }
}

pub struct Parser<'source> {
    lexer: Lexer<'source, Token>,
    file: SrcFile,
    accounts: HashMap<&'source str, Account>,
    sub_task_cond: Option<Arc<(Mutex<(VecDeque<(String, Source)>, usize)>, Condvar)>>,
    handlers: Option<Vec<std::thread::JoinHandle<Vec<(LedgerDraft, Vec<Error>)>>>>,
    tagset: HashSet<&'source str>,
}

impl<'source> Parser<'source> {
    fn src_from(&self, start: Location) -> Source {
        Source {
            start,
            end: self.lexer.last_token_end(),
            file: self.file.clone(),
        }
    }

    fn unexpected(&self, token: Token, text: &str) -> Result<(), Error> {
        Err(Error {
            level: ErrorLevel::Error,
            r#type: ErrorType::Syntax,
            msg: format!("Unexpected token {:?}({}).", token, text),
            src: Source {
                file: self.file.clone(),
                start: self.lexer.location(),
                end: self.lexer.location().advance(text.chars().count()),
            },
        })
    }

    fn parse_directives(&mut self, draft: &mut LedgerDraft, errors: &mut Vec<Error>) {
        while let Ok((token, text)) = self.lexer.peek() {
            let r = match token {
                Token::Include => self.parse_include(),
                Token::Option => self.parse_option(draft),
                Token::Commodity => self.parse_commodity(draft, None),
                Token::Date => self.parse_dated_entry(draft),
                Token::PushTag => self.parse_push_tag(),
                Token::PopTag => self.parse_pop_tag(),
                _ => self.unexpected(token, text),
            };
            if let Err(err) = r {
                errors.push(err);
                while let Ok((token, _)) = self.lexer.peek() {
                    match token {
                        Token::Option
                        | Token::Include
                        | Token::Date
                        | Token::PushTag
                        | Token::Commodity => break,
                        _ => self.lexer.consume(),
                    }
                }
            }
        }
    }

    fn sub_worker(
        _id: usize,
        cond: Arc<(Mutex<(VecDeque<(String, Source)>, usize)>, Condvar)>,
    ) -> Vec<(LedgerDraft, Vec<Error>)> {
        let mut sub_drafts = vec![];
        loop {
            let (lock, cvar) = cond.as_ref();
            let (task_path, refer_src) = {
                let mut changed = lock.lock().unwrap();
                while changed.0.len() == 0 && changed.1 > 0 {
                    changed = cvar.wait(changed).unwrap();
                }
                if changed.0.len() > 0 {
                    changed.1 += 1;
                    changed.0.pop_front().unwrap()
                } else {
                    cvar.notify_one();
                    return sub_drafts;
                }
            };
            let r = Self::parse_helper(task_path, refer_src, Some(cond.clone()));
            sub_drafts.push(r);
            {
                let num_thread = &mut lock.lock().unwrap().1;
                *num_thread -= 1;
            }
            cvar.notify_one();
        }
    }

    fn parse_push_tag(&mut self) -> Result<(), Error> {
        self.lexer.take(Token::PushTag)?;
        let start = self.lexer.location();
        let tag = self.lexer.take(Token::Tag)?;
        if self.tagset.insert(tag) {
            Ok(())
        } else {
            Err(Error {
                msg: format!("Duplicate tag {}.", tag),
                level: ErrorLevel::Info,
                r#type: ErrorType::Duplicate,
                src: self.src_from(start),
            })
        }
    }

    fn parse_pop_tag(&mut self) -> Result<(), Error> {
        self.lexer.take(Token::PopTag)?;
        let start = self.lexer.location();
        let tag = self.lexer.take(Token::Tag)?;
        if self.tagset.remove(&tag) {
            Ok(())
        } else {
            Err(Error {
                msg: format!("Tag {} does not exist.", tag),
                level: ErrorLevel::Info,
                r#type: ErrorType::NoMatch,
                src: self.src_from(start),
            })
        }
    }

    fn parse_include(&mut self) -> Result<(), Error> {
        let start = self.lexer.location();
        self.lexer.take(Token::Include)?;
        let path_str = self.parse_string()?;
        let path = Path::new(path_str);
        let mut path_buf = PathBuf::from(self.file.as_str());
        let full_path = if path.is_absolute() {
            path_str
        } else {
            path_buf.pop();
            path_buf.push(path);
            path_buf.as_path().to_str().unwrap()
        }
        .to_string();
        let src = self.src_from(start);
        if let Some(sub_task) = self.sub_task_cond.as_mut() {
            {
                (*sub_task).0.lock().unwrap().0.push_back((full_path, src));
            }
            (*sub_task).1.notify_one();
        } else {
            let mut q = VecDeque::new();
            q.push_back((full_path, src));
            let sub_task_cond = Arc::new((Mutex::new((q, 0)), Condvar::new()));
            self.sub_task_cond = Some(sub_task_cond.clone());
            let num_threads = std::env::var("LUMI_PARSER_THREADS")
                .ok()
                .and_then(|num| num.parse::<usize>().ok())
                .unwrap_or(num_cpus::get());
            let handlers = (1..num_threads)
                .map(|id| {
                    let cond = sub_task_cond.clone();
                    std::thread::spawn(move || Self::sub_worker(id, cond))
                })
                .collect::<Vec<_>>();
            self.handlers = Some(handlers);
        }

        Ok(())
    }

    fn parse_option(&mut self, draft: &mut LedgerDraft) -> Result<(), Error> {
        let start = self.lexer.location();
        self.lexer.take(Token::Option)?;
        let key = self.parse_string()?;
        let val = self.parse_string()?;
        let src = self.src_from(start);
        draft.add_option(key.to_string(), val.to_string(), src)
    }

    fn parse_commodity(
        &mut self,
        draft: &mut LedgerDraft,
        date: Option<&'source str>,
    ) -> Result<(), Error> {
        let start = self.lexer.location();
        self.lexer.take(Token::Commodity)?;
        let commodity = self.lexer.take(Token::Currency)?;
        let src = self.src_from(start);
        let mut meta = self.parse_meta()?;
        if let Some(date_str) = date {
            meta.insert("date".to_string(), (date_str.to_string(), src.clone()));
        }
        draft.add_commodity(commodity.to_string(), meta, src)?;
        Ok(())
    }

    fn parse_meta(&mut self) -> Result<Meta, Error> {
        let mut meta = Meta::new();
        while let Ok((Token::MetaLabel, key)) = self.lexer.peek() {
            let start = self.lexer.location();
            self.lexer.consume();
            let val = self.parse_string()?;
            meta.insert(key.to_string(), (val.to_string(), self.src_from(start)));
        }
        Ok(meta)
    }

    fn parse_dated_entry(&mut self, draft: &mut LedgerDraft) -> Result<(), Error> {
        let start = self.lexer.location();
        let date_str = self.lexer.take(Token::Date)?;
        let date = date_str.parse::<Date>().map_err(|_| Error {
            msg: format!("Invalid date: {}.", date_str),
            src: Source {
                file: self.file.clone(),
                start,
                end: self.lexer.location(),
            },
            r#type: ErrorType::Syntax,
            level: ErrorLevel::Error,
        })?;
        let (token, text) = self.lexer.peek()?;
        match token {
            Token::Asterisk | Token::QuestionMark | Token::Txn | Token::Balance | Token::Pad => {
                self.parse_txn(date, draft)
            }
            Token::Open => self.parse_open(date, draft),
            Token::Close => self.parse_close(date, draft),
            Token::Document => self.parse_document(date, draft),
            Token::Note => self.parse_note(date, draft),
            Token::Event => self.parse_event(date, draft),
            Token::Commodity => self.parse_commodity(draft, Some(date_str)),
            _ => self.unexpected(token, text),
        }
    }

    fn parse_event(&mut self, date: Date, draft: &mut LedgerDraft) -> Result<(), Error> {
        let start = self.lexer.location();
        self.lexer.take(Token::Event)?;
        let key = self.parse_string()?;
        let val = self.parse_string()?;
        let src = self.src_from(start);
        draft
            .events
            .entry(key.to_string())
            .or_insert(vec![])
            .push((date, val.to_string(), src).into());
        Ok(())
    }

    fn parse_note(&mut self, date: Date, draft: &mut LedgerDraft) -> Result<(), Error> {
        let start = self.lexer.location();
        self.lexer.take(Token::Note)?;
        let account = self.parse_account()?;
        let val = self.parse_string()?;
        let note = AccountNote {
            date,
            val: val.to_string(),
            src: self.src_from(start),
        };
        draft
            .accounts
            .entry(account)
            .or_insert(AccountInfoDraft::default())
            .notes
            .push(note);
        Ok(())
    }

    fn parse_document(&mut self, date: Date, draft: &mut LedgerDraft) -> Result<(), Error> {
        let start = self.lexer.location();
        self.lexer.take(Token::Document)?;
        let account = self.parse_account()?;
        let val = self.parse_string()?;
        let doc = AccountDoc {
            date,
            val: val.to_string(),
            src: self.src_from(start),
        };
        draft
            .accounts
            .entry(account)
            .or_insert(AccountInfoDraft::default())
            .docs
            .push(doc);
        Ok(())
    }

    fn parse_account(&mut self) -> Result<Account, Error> {
        let account_str = self.lexer.take(Token::Account)?;
        let account = self
            .accounts
            .entry(account_str)
            .or_insert(Arc::new(account_str.to_string()))
            .clone();
        Ok(account)
    }

    fn parse_open(&mut self, date: Date, draft: &mut LedgerDraft) -> Result<(), Error> {
        let start = self.lexer.location();
        self.lexer.take(Token::Open)?;
        let account = self.parse_account()?;
        let set = self.parse_currency_set()?;
        let meta = self.parse_meta()?;
        let info = draft
            .accounts
            .entry(account)
            .or_insert(AccountInfoDraft::default());
        info.open = Some((date, self.src_from(start)));
        info.currencies = set;
        info.meta = meta;
        Ok(())
    }

    fn parse_close(&mut self, date: Date, draft: &mut LedgerDraft) -> Result<(), Error> {
        let start = self.lexer.location();
        self.lexer.take(Token::Close)?;
        let account = self.parse_account()?;
        let info = draft
            .accounts
            .entry(account)
            .or_insert(AccountInfoDraft::default());
        info.close = Some((date, self.src_from(start)));
        Ok(())
    }

    fn parse_currency_set(&mut self) -> Result<HashSet<Currency>, Error> {
        let mut set = HashSet::new();
        if let Ok((Token::Currency, currency)) = self.lexer.peek() {
            set.insert(currency.to_string());
            self.lexer.consume();
            while let Ok((Token::Comma, _)) = self.lexer.peek() {
                self.lexer.consume();
                let currency = self.lexer.take(Token::Currency)?;
                set.insert(currency.to_string());
            }
        }
        Ok(set)
    }

    fn parse_txn(&mut self, date: Date, draft: &mut LedgerDraft) -> Result<(), Error> {
        let txn_start = self.lexer.location();
        let (token, text) = self.lexer.peek()?;
        let flag = match token {
            Token::Asterisk | Token::Txn => TxnFlag::Posted,
            Token::QuestionMark => TxnFlag::Pending,
            Token::Balance => TxnFlag::Balance,
            Token::Pad => TxnFlag::Pad,
            _ => return self.unexpected(token, text),
        };
        self.lexer.consume();
        let (payee, narration) = {
            let (token1, text1) = self.lexer.peek()?;
            if token1 == Token::String {
                self.lexer.consume();
                let (token2, text2) = self.lexer.peek()?;
                if token2 == Token::String {
                    self.lexer.consume();
                    (
                        Self::remove_quotes(text1).to_string(),
                        Self::remove_quotes(text2).to_string(),
                    )
                } else {
                    (String::new(), Self::remove_quotes(text1).to_string())
                }
            } else {
                (String::new(), String::new())
            }
        };

        let mut links = Vec::new();
        let mut tags = Vec::new();
        while let Ok((token, text)) = self.lexer.peek() {
            match token {
                Token::Link => links.push(text.to_string()),
                Token::Tag => tags.push(text.to_string()),
                _ => break,
            };
            self.lexer.consume();
        }
        if flag != TxnFlag::Balance {
            for tag in self.tagset.iter() {
                tags.push(tag.to_string());
            }
        }

        let meta = self.parse_meta()?;
        let postings = self.parse_postings()?;
        let src = self.src_from(txn_start);
        let txn = TxnDraft {
            date,
            flag,
            payee,
            narration,
            links,
            tags,
            meta,
            postings,
            src,
        };
        draft.txns.push(txn);
        Ok(())
    }

    fn parse_postings(&mut self) -> Result<Vec<PostingDraft>, Error> {
        let mut postings = Vec::new();
        while let Ok((Token::Account, _)) = self.lexer.peek() {
            let start = self.lexer.location();
            let account = self.parse_account()?;
            let amount;
            let cost;
            let price;
            if let Ok((Token::Number, _)) = self.lexer.peek() {
                amount = Some(self.parse_amount()?);
                cost = self.parse_cost()?;
                price = self.parse_price()?;
            } else {
                amount = None;
                cost = None;
                price = None;
            }
            let meta = self.parse_meta()?;
            let src = self.src_from(start);
            postings.push(PostingDraft {
                account,
                amount,
                cost,
                price,
                meta,
                src,
            });
        }
        Ok(postings)
    }

    fn parse_cost(&mut self) -> Result<Option<CostLiteral>, Error> {
        if let Ok((token, _)) = self.lexer.peek() {
            if token == Token::LBrace || token == Token::LLBrace {
                self.lexer.consume();
                let (amount, date) = self.parse_cost_basis()?;
                let basis = match amount {
                    None => None,
                    Some(amount) => match token {
                        Token::LBrace => Some(CostBasis::Unit(amount)),
                        _ => Some(CostBasis::Total(amount)),
                    },
                };
                match token {
                    Token::LBrace => {
                        self.lexer.take(Token::RBrace)?;
                    }
                    _ => {
                        self.lexer.take(Token::RRBrace)?;
                    }
                };
                Ok(Some(CostLiteral { basis, date }))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    #[inline]
    fn remove_quotes(input: &str) -> &str {
        let mut chars = input.chars();
        chars.next();
        chars.next_back();
        chars.as_str()
    }

    fn parse_string(&mut self) -> Result<&'source str, Error> {
        let quoted_str = self.lexer.take(Token::String)?;
        Ok(Self::remove_quotes(quoted_str))
    }

    fn parse_date(&mut self) -> Result<Date, Error> {
        let start = self.lexer.location();
        let date_str = self.lexer.take(Token::Date)?;
        let date = date_str.parse::<Date>().map_err(|_| {
            let src = self.src_from(start);
            Error {
                msg: format!("Invalid date: {}.", date_str),
                src,
                r#type: ErrorType::Syntax,
                level: ErrorLevel::Error,
            }
        })?;
        Ok(date)
    }

    fn parse_cost_basis(&mut self) -> Result<(Option<Amount>, Option<Date>), Error> {
        let mut amount = None;
        let mut date = None;
        if let Ok((Token::Number, _)) = self.lexer.peek() {
            amount = Some(self.parse_amount()?);
            if let Ok((Token::Comma, _)) = self.lexer.peek() {
                self.lexer.consume();
                date = Some(self.parse_date()?);
            }
        } else if let Ok((Token::Date, _)) = self.lexer.peek() {
            date = Some(self.parse_date()?);
        }
        Ok((amount, date))
    }

    fn parse_price(&mut self) -> Result<Option<Price>, Error> {
        if let Ok((token, _)) = self.lexer.peek() {
            if token == Token::AtUnit || token == Token::AtTotal {
                self.lexer.consume();
                let amount = self.parse_amount()?;
                return if token == Token::AtUnit {
                    Ok(Some(Price::Unit(amount)))
                } else {
                    Ok(Some(Price::Total(amount)))
                };
            }
        }
        Ok(None)
    }
    fn parse_amount(&mut self) -> Result<Amount, Error> {
        let start = self.lexer.location();
        let num_str = self.lexer.take(Token::Number)?;
        let number = num_str.parse::<Decimal>().map_err(|e| Error {
            msg: e.to_string(),
            src: self.src_from(start),
            level: ErrorLevel::Error,
            r#type: ErrorType::Syntax,
        })?;
        let currency = self.lexer.take(Token::Currency)?;
        Ok(Amount {
            number: number,
            currency: currency.to_string(),
        })
    }

    pub fn parse(path: &str) -> (LedgerDraft, Vec<Error>) {
        let src = Source {
            file: Arc::new(path.to_string()),
            start: Location { line: 1, col: 1 },
            end: Location { line: 1, col: 1 },
        };
        Self::parse_helper(path.to_string(), src, None)
    }

    fn parse_helper(
        path: String,
        refer_src: Source,
        sub_task_cond: Option<Arc<(Mutex<(VecDeque<(String, Source)>, usize)>, Condvar)>>,
    ) -> (LedgerDraft, Vec<Error>) {
        let mut draft = LedgerDraft::default();
        match fs::read_to_string(&path) {
            Ok(data) => {
                let file = Arc::new(path);
                let mut parser = Parser {
                    lexer: Lexer::new(&data, file.clone()),
                    file,
                    accounts: HashMap::new(),
                    sub_task_cond,
                    handlers: None,
                    tagset: HashSet::new(),
                };
                let mut errors = Vec::new();
                parser.parse_directives(&mut draft, &mut errors);
                if let Some(handlers) = parser.handlers.take() {
                    let own_results =
                        Self::sub_worker(0, parser.sub_task_cond.as_ref().unwrap().clone());
                    for (sub_draft, errs) in own_results {
                        errors.extend(errs);
                        let merge_errors = draft.merge(sub_draft);
                        errors.extend(merge_errors);
                    }
                    let _ = handlers
                        .into_iter()
                        .map(|handler| {
                            let results = handler.join().unwrap();
                            for (sub_draft, errs) in results {
                                errors.extend(errs);
                                let merge_errors = draft.merge(sub_draft);
                                errors.extend(merge_errors);
                            }
                        })
                        .collect::<Vec<_>>();
                }
                (draft, errors)
            }
            Err(io_error) => {
                let error = Error {
                    r#type: ErrorType::Io,
                    level: ErrorLevel::Error,
                    msg: format!("Couldn't read {}: {:?}", &path, io_error),
                    src: refer_src,
                };
                (draft, vec![error])
            }
        }
    }
}

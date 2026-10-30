use std::sync::{Arc, RwLock};

use anyhow::{Result, anyhow};
use gpui_kit::{App, AppContext as _, Task};
use lumi::web::{FilterOptions, JournalItem, TrieOptions, TrieTable, TrieTableRow};
use lumi::{ErrorLevel, Ledger, Source, edit, query};

use crate::source::{Completions, JournalPage, LedgerSource, Positions};

/// A ledger parsed in this process from a file on disk.
///
/// Parsing and querying run on GPUI's background executor; the parsed ledger
/// is shared behind a lock so a query never blocks the interface.
pub struct LocalLedger {
    path: Arc<str>,
    loaded: Arc<RwLock<Option<Loaded>>>,
}

struct Loaded {
    ledger: Ledger,
    errors: Vec<lumi::Error>,
}

impl LocalLedger {
    /// A ledger read from the file at `path`. Nothing is parsed until the
    /// first [`reload`](LedgerSource::reload).
    pub fn new(path: impl Into<Arc<str>>) -> Self {
        Self {
            path: path.into(),
            loaded: Arc::new(RwLock::new(None)),
        }
    }

    /// The path of the ledger file.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The number of errors the last reload reported, if any reload has
    /// completed.
    pub fn error_count(&self) -> Option<usize> {
        self.loaded.read().ok()?.as_ref().map(|loaded| {
            loaded
                .errors
                .iter()
                .filter(|error| error.level == ErrorLevel::Error)
                .count()
        })
    }

    /// Runs `query` against the parsed ledger on the background executor.
    fn query<T, F>(&self, cx: &mut App, query: F) -> Task<Result<T>>
    where
        T: Send + 'static,
        F: FnOnce(&Loaded) -> T + Send + 'static,
    {
        let loaded = self.loaded.clone();
        let path = self.path.clone();
        cx.background_spawn(async move {
            let guard = loaded
                .read()
                .map_err(|_| anyhow!("The ledger {path} is unavailable after a failed reload."))?;
            let loaded = guard
                .as_ref()
                .ok_or_else(|| anyhow!("The ledger {path} has not been loaded yet."))?;
            Ok(query(loaded))
        })
    }
}

impl LedgerSource for LocalLedger {
    fn reload(&self, cx: &mut App) -> Task<Result<()>> {
        let loaded = self.loaded.clone();
        let path = self.path.clone();
        cx.background_spawn(async move {
            if !std::path::Path::new(&*path).is_file() {
                return Err(anyhow!("Couldn't find the ledger file {path}."));
            }
            let (ledger, errors) = Ledger::from_file(&path);
            *loaded.write().map_err(|_| {
                anyhow!("The ledger {path} is unavailable after a failed reload.")
            })? = Some(Loaded { ledger, errors });
            Ok(())
        })
    }

    fn trie_table(
        &self,
        root_account: &str,
        options: &TrieOptions,
        cx: &mut App,
    ) -> Task<Result<Option<TrieTable<String>>>> {
        let root_account = root_account.to_string();
        let options = options.clone();
        self.query(cx, move |loaded| {
            query::trie_table(&loaded.ledger, &root_account, &options).map(owned_trie_table)
        })
    }

    fn journal(
        &self,
        account: Option<&str>,
        options: &FilterOptions,
        cx: &mut App,
    ) -> Task<Result<JournalPage>> {
        let account = account.map(str::to_string);
        let options = options.clone();
        self.query(cx, move |loaded| {
            let (items, total) = query::journal(&loaded.ledger, account.as_deref(), &options);
            let items = items
                .into_iter()
                .map(|item| JournalItem {
                    txn: Arc::new(item.txn.clone()),
                    balance: owned_amounts(item.balance),
                    changes: owned_amounts(item.changes),
                })
                .collect();
            (items, total)
        })
    }

    fn journal_page_of(
        &self,
        account: Option<&str>,
        options: &FilterOptions,
        target: &Source,
        cx: &mut App,
    ) -> Task<Result<Option<usize>>> {
        let account = account.map(str::to_string);
        let options = options.clone();
        let target = target.clone();
        self.query(cx, move |loaded| {
            query::journal_page_of(&loaded.ledger, account.as_deref(), &options, &target)
        })
    }

    fn positions(&self, cx: &mut App) -> Task<Result<Positions>> {
        self.query(cx, |loaded| query::positions(loaded.ledger.balance_sheet()))
    }

    fn errors(&self, cx: &mut App) -> Task<Result<Vec<lumi::Error>>> {
        self.query(cx, |loaded| loaded.errors.clone())
    }

    fn completions(&self, cx: &mut App) -> Task<Result<Completions>> {
        self.query(cx, |loaded| query::completions(&loaded.ledger))
    }

    fn files(&self, cx: &mut App) -> Task<Result<Vec<String>>> {
        self.query(cx, |loaded| query::source_files(&loaded.ledger))
    }

    fn file_text(&self, path: &str, cx: &mut App) -> Task<Result<String>> {
        let path = path.to_string();
        cx.background_spawn(async move { Ok(edit::file_text(&path)?) })
    }

    fn replace_file_text(
        &self,
        path: &str,
        original: String,
        text: String,
        cx: &mut App,
    ) -> Task<Result<()>> {
        let path = path.to_string();
        cx.background_spawn(async move { Ok(edit::replace_file_text(&path, &original, &text)?) })
    }

    fn entry_text(&self, src: &Source, cx: &mut App) -> Task<Result<String>> {
        let file = src.file.clone();
        let line = src.start.line;
        cx.background_spawn(async move { Ok(edit::entry_text(&file, line)?) })
    }

    fn replace_entry_text(
        &self,
        src: &Source,
        original: String,
        text: String,
        cx: &mut App,
    ) -> Task<Result<()>> {
        let file = src.file.clone();
        let line = src.start.line;
        cx.background_spawn(
            async move { Ok(edit::replace_entry_text(&file, line, &original, &text)?) },
        )
    }

    fn delete_entry_text(&self, src: &Source, original: String, cx: &mut App) -> Task<Result<()>> {
        let file = src.file.clone();
        let line = src.start.line;
        cx.background_spawn(async move { Ok(edit::delete_entry_text(&file, line, &original)?) })
    }

    fn append_entry_text(&self, file: &str, text: String, cx: &mut App) -> Task<Result<()>> {
        let file = file.to_string();
        cx.background_spawn(async move { Ok(edit::append_entry_text(&file, &text)?) })
    }
}

fn owned_amounts(
    amounts: std::collections::HashMap<&str, rust_decimal::Decimal>,
) -> std::collections::HashMap<String, rust_decimal::Decimal> {
    amounts
        .into_iter()
        .map(|(currency, number)| (currency.to_string(), number))
        .collect()
}

fn owned_trie_table(table: TrieTable<&str>) -> TrieTable<String> {
    TrieTable {
        rows: table
            .rows
            .into_iter()
            .map(|row| TrieTableRow {
                level: row.level,
                name: row.name.to_string(),
                numbers: row.numbers,
            })
            .collect(),
        currencies: table.currencies.into_iter().map(str::to_string).collect(),
    }
}

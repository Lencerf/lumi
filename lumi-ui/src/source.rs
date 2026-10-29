//! Where the user interface gets its ledger data from.
//!
//! Views never read a [`Ledger`](lumi::Ledger) directly. They ask a
//! [`LedgerSource`] and receive the same owned data types that `lumi serve`
//! sends over HTTP, so one set of views works whether the ledger is parsed
//! in-process ([`LocalLedger`]) or queried from a server.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use gpui_kit::{App, Task};
pub use lumi::web::Completions;
use lumi::web::{FilterOptions, JournalItem, Position, TrieOptions, TrieTable};
use lumi::{Source, Transaction};

#[cfg(not(target_family = "wasm"))]
mod local;

#[cfg(not(target_family = "wasm"))]
pub use local::LocalLedger;

/// A transaction in a journal, with the changes it makes to the journal's
/// account and the account's running balance afterwards (both empty in a
/// journal that is not about one account).
pub type JournalEntry = JournalItem<String, Arc<Transaction>>;

/// One page of a journal: the entries on it, and how many entries match the
/// query across all pages.
pub type JournalPage = (Vec<JournalEntry>, usize);

/// The positions each account holds, by account name.
pub type Positions = HashMap<String, Vec<Position>>;

/// A ledger the user interface can query.
///
/// Every method returns a GPUI [`Task`], so an implementation decides for
/// itself whether it answers from memory, from a background thread, or from
/// the network. Results are owned values; a query made before the first
/// successful [`reload`](Self::reload) fails.
pub trait LedgerSource: 'static {
    /// Reads the ledger again from wherever it comes from, replacing the data
    /// behind every later query.
    fn reload(&self, cx: &mut App) -> Task<Result<()>>;

    /// The account hierarchy below `root_account` (for example `Assets`) with
    /// its balances, laid out as table rows. `None` if no account starts
    /// with `root_account`.
    fn trie_table(
        &self,
        root_account: &str,
        options: &TrieOptions,
        cx: &mut App,
    ) -> Task<Result<Option<TrieTable<String>>>>;

    /// One page of the journal. With `account`, only transactions touching
    /// that account or its sub-accounts, each with the account's running
    /// balance. See [`lumi::query::journal`] for the options.
    fn journal(
        &self,
        account: Option<&str>,
        options: &FilterOptions,
        cx: &mut App,
    ) -> Task<Result<JournalPage>>;

    /// Every position of every account, including zero balances.
    fn positions(&self, cx: &mut App) -> Task<Result<Positions>>;

    /// The errors the last reload reported.
    fn errors(&self, cx: &mut App) -> Task<Result<Vec<lumi::Error>>>;

    /// Vocabulary for entry editing auto-completion (open accounts,
    /// currencies, payees, narrations, tags, and links).
    fn completions(&self, cx: &mut App) -> Task<Result<Completions>>;

    /// The text of the entry at `src` in its ledger file, as
    /// [`lumi::edit::entry_text`] reads it: the line the entry starts on and
    /// the indented lines after it, without the final line break.
    fn entry_text(&self, src: &Source, cx: &mut App) -> Task<Result<String>>;

    /// Replaces the text of the entry at `src` with `text`. `original` is
    /// what [`entry_text`](Self::entry_text) returned; if the entry's text
    /// has changed since, nothing is written and the task fails. The ledger
    /// is not reloaded.
    fn replace_entry_text(
        &self,
        src: &Source,
        original: String,
        text: String,
        cx: &mut App,
    ) -> Task<Result<()>>;

    /// Deletes the entry at `src` from its ledger file. `original` is what
    /// [`entry_text`](Self::entry_text) returned; if the entry's text has
    /// changed since, nothing is written and the task fails. The ledger is
    /// not reloaded.
    fn delete_entry_text(&self, src: &Source, original: String, cx: &mut App) -> Task<Result<()>>;

    /// Appends `text` as a new entry at the end of `file`, separated by a
    /// blank line. The ledger is not reloaded.
    fn append_entry_text(&self, file: &str, text: String, cx: &mut App) -> Task<Result<()>>;
}

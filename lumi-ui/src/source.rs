//! Where the user interface gets its ledger data from.
//!
//! Views never read a [`Ledger`](lumi::Ledger) directly. They ask a
//! [`LedgerSource`] and receive the same owned data types that `lumi serve`
//! sends over HTTP, so one set of views works whether the ledger is parsed
//! in-process ([`LocalLedger`]) or queried from a server.

use anyhow::Result;
use gpui_kit::{App, Task};
use lumi::web::{TrieOptions, TrieTable};

#[cfg(not(target_family = "wasm"))]
mod local;

#[cfg(not(target_family = "wasm"))]
pub use local::LocalLedger;

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
}

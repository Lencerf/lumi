use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState,
};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, AppContext as _, Context, Div, ElementId, Entity, EventEmitter,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Stateful,
    Styled as _, Subscription, Task, Window, div, px,
};
use lumi::TxnFlag;
use lumi::web::FilterOptions;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::source::{JournalEntry, LedgerSource};

/// Opens the journal of an account, from a posting's context menu.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct OpenAccount(pub SharedString);

/// Shows or hides the postings of the transaction at the given entry index.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct TogglePostings(pub usize);

/// Activates the selected row: toggles a transaction's postings, or opens a
/// posting's account. Bound to Enter.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct ActivateRow;

/// The key context of a journal; its key bindings apply within it.
pub(crate) const KEY_CONTEXT: &str = "JournalReport";

/// How many transactions one request asks for.
const PAGE_SIZE: usize = 100;

/// The journal of the whole ledger, or of one account and its sub-accounts.
///
/// The table lists transactions newest first (the Date header flips the
/// order) and fetches further pages as the user scrolls. Each transaction
/// can show its postings, one row each; a posting row opens its account on
/// Enter, double click, or from its context menu.
pub struct JournalReport {
    page: Page,
    session: Entity<LedgerSession>,
    table: Entity<TableState<JournalDelegate>>,
    /// The ledger revision the table was last reset for.
    requested_revision: Option<u64>,
    _subscriptions: Vec<Subscription>,
}

/// One table row: a transaction, or one of its postings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JournalRow {
    Transaction(usize),
    Posting(usize, usize),
}

struct JournalDelegate {
    source: Arc<dyn LedgerSource>,
    account: Option<SharedString>,
    columns: Vec<Column>,
    entries: Vec<JournalEntry>,
    rows: Vec<JournalRow>,
    expanded: HashSet<usize>,
    expand_all: bool,
    old_first: bool,
    /// How many transactions match, across all pages.
    total: usize,
    loaded_pages: usize,
    loading: bool,
    failed: Option<SharedString>,
    /// Counts resets so that a page that arrives for an earlier one is dropped.
    generation: u64,
    _load_task: Option<Task<()>>,
}

impl JournalReport {
    pub fn new(
        page: Page,
        session: Entity<LedgerSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = session.read(cx).source().clone();
        let delegate = JournalDelegate::new(source, page.account().cloned());
        let table = cx.new(|cx| TableState::new(delegate, window, cx));
        let _subscriptions = vec![
            cx.observe(&session, |this, _, cx| this.load(cx)),
            cx.subscribe(&table, Self::on_table_event),
        ];
        let mut this = Self {
            page,
            session,
            table,
            requested_revision: None,
            _subscriptions,
        };
        this.load(cx);
        this
    }

    pub fn page(&self) -> &Page {
        &self.page
    }

    /// The transactions loaded so far, in table order.
    pub fn entries(&self, cx: &App) -> Vec<JournalEntry> {
        self.table.read(cx).delegate().entries.clone()
    }

    /// How many transactions match, across all pages.
    pub fn total(&self, cx: &App) -> usize {
        self.table.read(cx).delegate().total
    }

    pub fn is_showing_postings(&self, cx: &App) -> bool {
        self.table.read(cx).delegate().expand_all
    }

    /// Starts the table over for the current ledger revision, unless that
    /// has already been done.
    fn load(&mut self, cx: &mut Context<Self>) {
        let revision = {
            let session = self.session.read(cx);
            if !session.is_loaded() {
                return;
            }
            session.revision()
        };
        if self.requested_revision == Some(revision) {
            return;
        }
        self.requested_revision = Some(revision);
        self.table.update(cx, |table, cx| {
            table.delegate_mut().reset();
            table.delegate_mut().load_next_page(cx);
            cx.notify();
        });
    }

    fn set_show_postings(&mut self, show: bool, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if delegate.expand_all != show {
                delegate.expand_all = show;
                delegate.rebuild_rows();
                cx.notify();
            }
        });
    }

    fn toggle_postings(&mut self, entry_ix: usize, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if !delegate.expanded.remove(&entry_ix) {
                delegate.expanded.insert(entry_ix);
            }
            delegate.rebuild_rows();
            cx.notify();
        });
    }

    fn open_account(&mut self, account: SharedString, cx: &mut Context<Self>) {
        cx.emit(ReportEvent::OpenAccount(account));
    }

    /// Toggles a transaction row's postings, or opens a posting row's account.
    fn activate_row(&mut self, row_ix: usize, cx: &mut Context<Self>) {
        let row = self.table.read(cx).delegate().rows.get(row_ix).copied();
        match row {
            Some(JournalRow::Transaction(entry_ix)) => {
                let is_balance =
                    self.table.read(cx).delegate().entries[entry_ix].txn.flag() == TxnFlag::Balance;
                if !is_balance {
                    self.toggle_postings(entry_ix, cx);
                }
            }
            Some(JournalRow::Posting(entry_ix, posting_ix)) => {
                let account: SharedString = self.table.read(cx).delegate().entries[entry_ix]
                    .txn
                    .postings()[posting_ix]
                    .account
                    .to_string()
                    .into();
                self.open_account(account, cx);
            }
            None => {}
        }
    }

    fn activate_selected_row(&mut self, _: &ActivateRow, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(row_ix) = self.table.read(cx).selected_row() {
            self.activate_row(row_ix, cx);
        }
    }

    fn on_open_account(&mut self, action: &OpenAccount, _: &mut Window, cx: &mut Context<Self>) {
        self.open_account(action.0.clone(), cx);
    }

    fn on_toggle_postings(
        &mut self,
        action: &TogglePostings,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_postings(action.0, cx);
    }

    fn on_table_event(
        &mut self,
        _: Entity<TableState<JournalDelegate>>,
        event: &TableEvent,
        cx: &mut Context<Self>,
    ) {
        if let TableEvent::DoubleClickedRow(row_ix) = event {
            self.activate_row(*row_ix, cx);
        }
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().failed = None;
            table.delegate_mut().load_next_page(cx);
            cx.notify();
        });
    }
}

impl EventEmitter<ReportEvent> for JournalReport {}

impl Render for JournalReport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (loaded, total, expand_all, failed) = {
            let delegate = self.table.read(cx).delegate();
            (
                delegate.entries.len(),
                delegate.total,
                delegate.expand_all,
                delegate.failed.clone(),
            )
        };
        let summary: SharedString = if total == 1 {
            "1 transaction".into()
        } else if loaded < total {
            format!("{loaded} of {total} transactions").into()
        } else {
            format!("{total} transactions").into()
        };
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::activate_selected_row))
            .on_action(cx.listener(Self::on_open_account))
            .on_action(cx.listener(Self::on_toggle_postings))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_6()
                    .pt_4()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(summary),
                    )
                    .child(
                        Checkbox::new((ElementId::from("show-postings"), self.page.id()))
                            .label("Show postings")
                            .checked(expand_all)
                            .on_change(cx.listener(|this, checked, _, cx| {
                                this.set_show_postings(*checked, cx)
                            })),
                    ),
            )
            .when_some(failed, |this, message| {
                this.child(
                    div()
                        .v_flex()
                        .items_start()
                        .gap_2()
                        .px_6()
                        .pt_3()
                        .child(
                            Alert::error("journal-failed", message)
                                .title("Couldn't read the journal"),
                        )
                        .child(
                            Button::new("journal-retry")
                                .label("Retry")
                                .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p_6()
                    .pt_3()
                    .child(DataTable::new(&self.table).small().stripe(true)),
            )
    }
}

impl JournalDelegate {
    fn new(source: Arc<dyn LedgerSource>, account: Option<SharedString>) -> Self {
        // A posting's cost and price share the Position column in beancount
        // notation (`10 VTI {250.00 USD, 2024-03-01} @ 1.20 EUR`), so the
        // running balance of an account page stays in view.
        let mut columns = vec![
            Column::new("date", "Date").width(px(104.)).sortable(),
            Column::new("flag", "Flag").width(px(48.)).text_center(),
            Column::new("description", "Description").width(px(if account.is_some() {
                280.
            } else {
                340.
            })),
            Column::new("position", "Position")
                .width(px(230.))
                .text_right(),
        ];
        if account.is_some() {
            columns.push(Column::new("change", "Change").width(px(150.)).text_right());
            columns.push(
                Column::new("balance", "Balance")
                    .width(px(170.))
                    .text_right(),
            );
        }
        Self {
            source,
            account,
            columns,
            entries: Vec::new(),
            rows: Vec::new(),
            expanded: HashSet::new(),
            expand_all: false,
            old_first: false,
            total: 0,
            loaded_pages: 0,
            loading: false,
            failed: None,
            generation: 0,
            _load_task: None,
        }
    }

    /// Forgets every loaded page. The next [`load_next_page`](Self::load_next_page)
    /// starts from the first page again.
    fn reset(&mut self) {
        self.entries.clear();
        self.rows.clear();
        self.expanded.clear();
        self.total = 0;
        self.loaded_pages = 0;
        self.loading = false;
        self.failed = None;
        self.generation += 1;
        self._load_task = None;
    }

    fn load_next_page(&mut self, cx: &mut Context<TableState<Self>>) {
        if self.loading {
            return;
        }
        self.loading = true;
        let options = FilterOptions {
            entries: Some(PAGE_SIZE),
            page: Some(self.loaded_pages + 1),
            old_first: Some(self.old_first),
            ..Default::default()
        };
        let task = self.source.journal(self.account.as_deref(), &options, cx);
        let generation = self.generation;
        self._load_task = Some(cx.spawn(async move |table, cx| {
            let result = task.await;
            table
                .update(cx, |table, cx| {
                    let delegate = table.delegate_mut();
                    if delegate.generation != generation {
                        return;
                    }
                    delegate.loading = false;
                    match result {
                        Ok((entries, total)) => {
                            delegate.total = total;
                            delegate.loaded_pages += 1;
                            delegate.entries.extend(entries);
                            delegate.rebuild_rows();
                        }
                        Err(error) => delegate.failed = Some(format!("{error:#}").into()),
                    }
                    cx.notify();
                })
                .ok();
        }));
    }

    /// Lays the entries out as rows: a transaction, then its postings when
    /// they are shown. A balance assertion has nothing to expand.
    fn rebuild_rows(&mut self) {
        self.rows.clear();
        for (entry_ix, entry) in self.entries.iter().enumerate() {
            self.rows.push(JournalRow::Transaction(entry_ix));
            let is_balance = entry.txn.flag() == TxnFlag::Balance;
            if !is_balance && (self.expand_all || self.expanded.contains(&entry_ix)) {
                for posting_ix in 0..entry.txn.postings().len() {
                    self.rows.push(JournalRow::Posting(entry_ix, posting_ix));
                }
            }
        }
    }

    fn is_expanded(&self, entry_ix: usize) -> bool {
        self.expand_all || self.expanded.contains(&entry_ix)
    }
}

/// Non-zero amounts as `number currency`, joined by commas, by currency.
fn amounts_text(amounts: &HashMap<String, Decimal>) -> String {
    let mut parts: Vec<_> = amounts
        .iter()
        .filter(|(_, number)| !number.is_zero())
        .collect();
    parts.sort_by(|a, b| a.0.cmp(b.0));
    parts
        .into_iter()
        .map(|(currency, number)| format!("{number} {currency}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A posting's amount with its cost and price, as beancount writes them:
/// `10 VTI {250.00 USD, 2024-03-01} @ 1.20 EUR`.
fn position_text(posting: &lumi::Posting) -> String {
    let mut text = posting.amount.to_string();
    if let Some(cost) = &posting.cost {
        text.push_str(&format!(" {{{}, {}}}", cost.amount, cost.date));
    }
    if let Some(price) = &posting.price {
        text.push_str(&format!(" @ {price}"));
    }
    text
}

fn flag_text(flag: TxnFlag) -> &'static str {
    match flag {
        TxnFlag::Posted => "*",
        TxnFlag::Pending => "!",
        TxnFlag::Pad => "pad",
        TxnFlag::Balance => "bal",
    }
}

impl TableDelegate for JournalDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if self.columns[col_ix].key.as_ref() != "date" {
            return;
        }
        let old_first = sort == ColumnSort::Ascending;
        if self.old_first != old_first {
            self.old_first = old_first;
            self.reset();
            self.load_next_page(cx);
        }
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        // The table also asks for the striped filler rows past the last row.
        let row = div().id(("row", row_ix));
        match self.rows.get(row_ix) {
            Some(JournalRow::Posting(..)) => row.text_color(cx.theme().muted_foreground),
            _ => row,
        }
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        match self.rows.get(row_ix) {
            Some(&JournalRow::Transaction(entry_ix))
                if self.entries[entry_ix].txn.flag() != TxnFlag::Balance =>
            {
                let label = if self.is_expanded(entry_ix) {
                    "Hide postings"
                } else {
                    "Show postings"
                };
                menu.menu(label, Box::new(TogglePostings(entry_ix)))
            }
            Some(&JournalRow::Posting(entry_ix, posting_ix)) => {
                let account = self.entries[entry_ix].txn.postings()[posting_ix]
                    .account
                    .to_string();
                menu.menu("Open account", Box::new(OpenAccount(account.into())))
            }
            _ => menu,
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let column = self.columns[col_ix].key.as_ref();
        match self.rows[row_ix] {
            JournalRow::Transaction(entry_ix) => {
                let entry = &self.entries[entry_ix];
                let txn = &entry.txn;
                match column {
                    "date" => div()
                        .font_family(mono)
                        .child(txn.date().to_string())
                        .into_any_element(),
                    "flag" => div()
                        .font_family(mono)
                        .child(flag_text(txn.flag()))
                        .into_any_element(),
                    "description" => {
                        if txn.flag() == TxnFlag::Balance {
                            let account = txn
                                .postings()
                                .first()
                                .map(|posting| posting.account.to_string())
                                .unwrap_or_default();
                            div().child(account).into_any_element()
                        } else {
                            let expanded = self.is_expanded(entry_ix);
                            div()
                                .h_flex()
                                .gap_1()
                                .child(
                                    Button::new(("postings", entry_ix))
                                        .ghost()
                                        .xsmall()
                                        .compact()
                                        .icon(if expanded {
                                            gpui_kit::component::IconName::ChevronDown
                                        } else {
                                            gpui_kit::component::IconName::ChevronRight
                                        })
                                        .accessibility_label(if expanded {
                                            "Hide postings"
                                        } else {
                                            "Show postings"
                                        })
                                        .on_click(cx.listener(move |table, _, _, cx| {
                                            let delegate = table.delegate_mut();
                                            if !delegate.expanded.remove(&entry_ix) {
                                                delegate.expanded.insert(entry_ix);
                                            }
                                            delegate.rebuild_rows();
                                            cx.notify();
                                        })),
                                )
                                .when(!txn.payee().is_empty(), |this| {
                                    this.child(div().font_semibold().child(txn.payee().clone()))
                                })
                                .when(!txn.narration().is_empty(), |this| {
                                    this.child(txn.narration().clone())
                                })
                                .into_any_element()
                        }
                    }
                    "position" if txn.flag() == TxnFlag::Balance => div()
                        .font_family(mono)
                        .child(
                            txn.postings()
                                .first()
                                .map(|posting| posting.amount.to_string())
                                .unwrap_or_default(),
                        )
                        .into_any_element(),
                    "change" => div()
                        .font_family(mono)
                        .child(amounts_text(&entry.changes))
                        .into_any_element(),
                    "balance" => div()
                        .font_family(mono)
                        .child(amounts_text(&entry.balance))
                        .into_any_element(),
                    _ => div().into_any_element(),
                }
            }
            JournalRow::Posting(entry_ix, posting_ix) => {
                let posting = &self.entries[entry_ix].txn.postings()[posting_ix];
                match column {
                    "description" => div()
                        .pl_6()
                        .child(posting.account.to_string())
                        .into_any_element(),
                    "position" => div()
                        .font_family(mono)
                        .child(position_text(posting))
                        .into_any_element(),
                    _ => div().into_any_element(),
                }
            }
        }
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, _: &App) -> Option<SharedString> {
        let column = self.columns.get(col_ix)?.key.as_ref();
        let text = match *self.rows.get(row_ix)? {
            JournalRow::Transaction(entry_ix) => {
                let entry = &self.entries[entry_ix];
                match column {
                    "date" => entry.txn.date().to_string(),
                    "flag" => flag_text(entry.txn.flag()).to_string(),
                    "description" => format!("{} {}", entry.txn.payee(), entry.txn.narration())
                        .trim()
                        .to_string(),
                    "change" => amounts_text(&entry.changes),
                    "balance" => amounts_text(&entry.balance),
                    _ => String::new(),
                }
            }
            JournalRow::Posting(entry_ix, posting_ix) => {
                let posting = &self.entries[entry_ix].txn.postings()[posting_ix];
                match column {
                    "description" => posting.account.to_string(),
                    "position" => position_text(posting),
                    _ => String::new(),
                }
            }
        };
        Some(text.into())
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .h_flex()
            .size_full()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(if self.loading { "" } else { "No transactions" })
    }

    fn loading(&self, _: &App) -> bool {
        self.loading
    }

    fn has_more(&self, _: &App) -> bool {
        self.failed.is_none() && self.entries.len() < self.total
    }

    fn load_more_threshold(&self) -> usize {
        50
    }

    fn load_more(&mut self, _: &mut Window, cx: &mut Context<TableState<Self>>) {
        self.load_next_page(cx);
    }
}

#[cfg(test)]
#[path = "journal_report_test.rs"]
mod tests;

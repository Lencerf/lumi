use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Search};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::pagination::Pagination;
use gpui_kit::component::table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState,
};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, Size, StyledExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, AppContext as _, ClickEvent, Context, Div, ElementId, Entity, EventEmitter,
    Focusable as _, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render, Role,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    TestSupportExt as _, Window, div, px,
};
use lumi::web::{DEFAULT_ENTRIES_PER_PAGE, FilterOptions};
use lumi::{Source, TxnFlag};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::entry_editor::EntryEditor;
use crate::input_guard::EditDragGuard;
use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::source::{JournalEntry, LedgerSource};
use crate::style::{
    interactive_button, table_card, table_focused_row_bg, table_header, table_row_bg,
};

/// Opens the journal of an account, from a posting's context menu.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct OpenAccount(pub SharedString);

/// Shows or hides the postings of the transaction at the given entry index.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct TogglePostings(pub usize);

/// Opens the editor on the transaction at the given entry index, from its
/// date or its context menu.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct EditEntry(pub usize);

/// Clears active filters, jumps to the unfiltered page containing the
/// transaction at the given entry index, and selects its row.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct ShowInContext(pub usize);

/// Navigates from an account journal to the main Journal page and shows the
/// transaction at the given entry index.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct ShowInJournal(pub usize);

/// Activates the selected row: toggles a transaction's postings, or opens a
/// posting's account. Bound to Enter.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = lumi_ui, no_json)]
pub struct ActivateRow;

/// The key context of a journal; its key bindings apply within it.
pub(crate) const KEY_CONTEXT: &str = "JournalReport";

/// The page sizes the user can choose from.
pub const PAGE_SIZES: [usize; 3] = [20, 50, 100];

/// Selectable record types in the journal and account views, matching Fava's
/// entry-type toggle strip (`*` cleared, `!` pending, `bal` balance assertions,
/// `pad` directives).
pub const RECORD_TYPES: [(TxnFlag, &str, &str); 4] = [
    (TxnFlag::Posted, "*", "Cleared transactions (*)"),
    (TxnFlag::Pending, "!", "Pending transactions (!)"),
    (TxnFlag::Balance, "bal", "Balance assertions (balance)"),
    (TxnFlag::Pad, "pad", "Pad directives (pad)"),
];

/// The journal of the whole ledger, or of one account and its sub-accounts.
///
/// The table shows one page of transactions, newest first (the Date header
/// flips the order), with pagination below it. Each transaction can show
/// its postings, one row each; a posting row opens its account on Enter,
/// double click, or from its context menu.
pub struct JournalReport {
    page: Page,
    session: Entity<LedgerSession>,
    table: Entity<TableState<JournalDelegate>>,
    search_input: Entity<InputState>,
    search_drag_guard: EditDragGuard,
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
    query: SharedString,
    visible_flags: HashSet<TxnFlag>,
    /// The last record the user clicked or right-clicked in this table.
    focused_entry: Option<Source>,
    /// The page shown, counted from 1.
    page: usize,
    page_size: usize,
    /// How many transactions match, across all pages.
    total: usize,
    loading: bool,
    failed: Option<SharedString>,
    /// Counts requests so that a page that arrives for an earlier one is
    /// dropped.
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
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search date, account, payee, narration, currency…")
                .clean_on_escape()
        });
        let search_drag_guard = EditDragGuard::new(&search_input);
        let _subscriptions = vec![
            cx.observe(&session, |this, _, cx| this.load(cx)),
            cx.subscribe(&table, Self::on_table_event),
            cx.subscribe(&search_input, Self::on_search_input_event),
        ];
        let mut this = Self {
            page,
            session,
            table,
            search_input,
            search_drag_guard,
            requested_revision: None,
            _subscriptions,
        };
        this.load(cx);
        this
    }

    pub fn page(&self) -> &Page {
        &self.page
    }

    /// The transactions on the page shown, in table order.
    pub fn entries(&self, cx: &App) -> Vec<JournalEntry> {
        self.table.read(cx).delegate().entries.clone()
    }

    /// How many transactions match, across all pages.
    pub fn total(&self, cx: &App) -> usize {
        self.table.read(cx).delegate().total
    }

    /// The page shown, counted from 1.
    pub fn page_number(&self, cx: &App) -> usize {
        self.table.read(cx).delegate().page
    }

    pub fn page_size(&self, cx: &App) -> usize {
        self.table.read(cx).delegate().page_size
    }

    pub fn is_showing_postings(&self, cx: &App) -> bool {
        self.table.read(cx).delegate().expand_all
    }

    pub fn search_query(&self, cx: &App) -> SharedString {
        self.table.read(cx).delegate().query.clone()
    }

    /// The source location of the last record clicked or right-clicked by the user.
    pub fn focused_entry(&self, cx: &App) -> Option<Source> {
        self.table.read(cx).delegate().focused_entry.clone()
    }

    pub fn is_search_focused(&self, window: &Window, cx: &App) -> bool {
        self.search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }

    #[cfg(test)]
    pub(crate) fn search_input_state(&self) -> &Entity<InputState> {
        &self.search_input
    }

    /// Focuses the search box and selects any existing query text.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_drag_guard.cancel();
        self.search_input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    /// Sets the search query and reloads the first page of matching entries.
    pub fn set_search_query(
        &mut self,
        query: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query: SharedString = query.into();
        self.search_input
            .update(cx, |input, cx| input.set_value(query.clone(), window, cx));
        self.apply_search_query(query, cx);
    }

    fn apply_search_query(&mut self, query: SharedString, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if delegate.query != query {
                delegate.query = query;
                delegate.load_filtered_page(cx);
            }
        });
    }

    fn on_search_input_event(
        &mut self,
        input: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.search_drag_guard.cancel();
            let query = input.read(cx).value();
            self.apply_search_query(query, cx);
        }
    }

    pub fn visible_flags(&self, cx: &App) -> HashSet<TxnFlag> {
        self.table.read(cx).delegate().visible_flags.clone()
    }

    /// Sets which record flags (`*`, `!`, `bal`, `pad`) are shown and reloads
    /// the matching page (preferring the focused record's page when still visible).
    pub fn set_visible_flags(
        &mut self,
        flags: impl IntoIterator<Item = TxnFlag>,
        cx: &mut Context<Self>,
    ) {
        let next: HashSet<TxnFlag> = flags.into_iter().collect();
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if delegate.visible_flags != next {
                delegate.visible_flags = next;
                delegate.load_filtered_page(cx);
            }
        });
    }

    fn toggle_visible_flag(&mut self, flag: TxnFlag, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if !delegate.visible_flags.remove(&flag) {
                delegate.visible_flags.insert(flag);
            }
            delegate.load_filtered_page(cx);
        });
    }

    fn render_record_type_selector(
        &self,
        visible_flags: &HashSet<TxnFlag>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let filtered = visible_flags.len() < RECORD_TYPES.len();
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .child(
                ButtonGroup::new((ElementId::from("journal-record-types"), self.page.id()))
                    .multiple(true)
                    .outline()
                    .compact()
                    .small()
                    .children(RECORD_TYPES.map(|(flag, label, description)| {
                        Button::new((ElementId::from("record-type"), label))
                            .font_family(mono.clone())
                            .label(label)
                            .tooltip(description)
                            .selected(visible_flags.contains(&flag))
                            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                if event.click_count() >= 2 {
                                    this.set_visible_flags([flag], cx);
                                } else {
                                    this.toggle_visible_flag(flag, cx);
                                }
                            }))
                    })),
            )
            .child(if filtered {
                Button::new((ElementId::from("journal-record-types-all"), self.page.id()))
                    .ghost()
                    .xsmall()
                    .label("All")
                    .tooltip("Show all record types")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_visible_flags(RECORD_TYPES.map(|(flag, _, _)| flag), cx);
                    }))
            } else {
                Button::new((ElementId::from("journal-record-types-none"), self.page.id()))
                    .ghost()
                    .xsmall()
                    .label("None")
                    .tooltip("Hide all record types")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_visible_flags([], cx);
                    }))
            })
    }

    /// Reads the page again for the current ledger revision, unless that has
    /// already been done.
    fn load(&mut self, cx: &mut Context<Self>) {
        let (revision, source) = {
            let session = self.session.read(cx);
            if !session.is_loaded() {
                return;
            }
            (session.revision(), session.source().clone())
        };
        if self.requested_revision == Some(revision) {
            return;
        }
        self.requested_revision = Some(revision);
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.source = source;
            delegate.load_page(cx);
        });
    }

    fn open_page_number(&mut self, page: usize, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if delegate.page != page {
                delegate.page = page;
                delegate.load_page(cx);
            }
        });
    }

    fn set_page_size(&mut self, page_size: usize, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if delegate.page_size != page_size {
                // Keep the first transaction of the page in view.
                let first_ix = (delegate.page - 1) * delegate.page_size;
                delegate.page_size = page_size;
                delegate.page = first_ix / page_size + 1;
                delegate.load_page(cx);
            }
        });
    }

    fn set_show_postings(&mut self, show: bool, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if delegate.expand_all != show || (!show && !delegate.expanded.is_empty()) {
                delegate.expand_all = show;
                delegate.expanded.clear();
                delegate.rebuild_rows();
                sync_focused_selection(table, false, cx);
                cx.notify();
            }
        });
    }

    fn toggle_postings(&mut self, entry_ix: usize, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().toggle_entry(entry_ix);
            sync_focused_selection(table, false, cx);
            cx.notify();
        });
    }

    fn open_account(&mut self, account: SharedString, cx: &mut Context<Self>) {
        cx.emit(ReportEvent::OpenAccount(account));
    }

    /// Toggles a transaction row's postings, or opens a posting row's account.
    fn activate_row(&mut self, row_ix: usize, cx: &mut Context<Self>) {
        let row = self.table.update(cx, |table, _| {
            let delegate = table.delegate_mut();
            delegate.focus_row(row_ix);
            delegate.rows.get(row_ix).copied()
        });
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
        self.table.update(cx, |table, _| {
            table.delegate_mut().focus_entry(action.0);
        });
        self.toggle_postings(action.0, cx);
    }

    fn on_edit_entry(&mut self, action: &EditEntry, window: &mut Window, cx: &mut Context<Self>) {
        let txn = self.table.update(cx, |table, cx| {
            table.delegate_mut().focus_entry(action.0);
            sync_focused_selection(table, false, cx);
            table.delegate().entries[action.0].txn.clone()
        });
        EntryEditor::open(&txn, self.session.clone(), window, cx);
    }

    /// Clears the search query and record-type filter, navigates to the page
    /// containing `source`, and selects its row in the table.
    pub fn show_entry_in_context(
        &mut self,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.focused_entry = Some(source);
            delegate.query = SharedString::default();
            delegate.visible_flags = RECORD_TYPES.iter().map(|(flag, _, _)| *flag).collect();
            delegate.load_filtered_page(cx);
        });
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        let table_focus = self.table.read(cx).focus_handle(cx);
        table_focus.focus(window, cx);
    }

    /// Clears the search query and record-type filter, navigates to the page
    /// containing `entry_ix`, and selects its row in the table.
    pub fn show_in_context(
        &mut self,
        entry_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self
            .table
            .read(cx)
            .delegate()
            .entries
            .get(entry_ix)
            .map(|entry| entry.txn.src().clone())
        else {
            return;
        };
        self.show_entry_in_context(source, window, cx);
    }

    fn on_show_in_context(
        &mut self,
        action: &ShowInContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_in_context(action.0, window, cx);
    }

    fn on_show_in_journal(
        &mut self,
        action: &ShowInJournal,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.table.update(cx, |table, _| {
            let delegate = table.delegate_mut();
            delegate.focus_entry(action.0);
            delegate
                .entries
                .get(action.0)
                .map(|entry| entry.txn.src().clone())
        }) else {
            return;
        };
        cx.emit(ReportEvent::ShowInJournal(source));
    }

    fn on_table_event(
        &mut self,
        table: Entity<TableState<JournalDelegate>>,
        event: &TableEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TableEvent::SelectRow(row_ix) => {
                table.update(cx, |table, cx| {
                    if table.delegate_mut().focus_row(*row_ix) {
                        cx.notify();
                    }
                });
            }
            TableEvent::RightClickedRow(Some(row_ix)) => {
                table.update(cx, |table, cx| {
                    table.delegate_mut().focus_row(*row_ix);
                    if table.selected_row().is_some() {
                        table.clear_selection(cx);
                    }
                    cx.notify();
                });
            }
            TableEvent::DoubleClickedRow(row_ix) => {
                self.activate_row(*row_ix, cx);
            }
            _ => {}
        }
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.table
            .update(cx, |table, cx| table.delegate_mut().load_page(cx));
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (page, page_size, total) = {
            let delegate = self.table.read(cx).delegate();
            (delegate.page, delegate.page_size, delegate.total)
        };
        let total_pages = total.div_ceil(page_size).max(1);
        div()
            .h_flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_6()
            .pb_4()
            .child(
                Pagination::new((ElementId::from("journal-pages"), self.page.id()))
                    .small()
                    .current_page(page)
                    .total_pages(total_pages)
                    .on_click(cx.listener(|this, page, _, cx| this.open_page_number(*page, cx))),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("Rows per page"),
                    )
                    .child(
                        ButtonGroup::new((ElementId::from("journal-page-size"), self.page.id()))
                            .outline()
                            .compact()
                            .children(PAGE_SIZES.map(|size| {
                                Button::new(("page-size", size))
                                    .xsmall()
                                    .label(size.to_string())
                                    .selected(size == page_size)
                            }))
                            .on_click(cx.listener(|this, selected: &Vec<usize>, _, cx| {
                                if let Some(size) =
                                    selected.first().and_then(|ix| PAGE_SIZES.get(*ix))
                                {
                                    this.set_page_size(*size, cx);
                                }
                            })),
                    ),
            )
    }
}

impl EventEmitter<ReportEvent> for JournalReport {}

impl Render for JournalReport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (first, last, total, expand_all, visible_flags, failed) = {
            let delegate = self.table.read(cx).delegate();
            let first = (delegate.page - 1) * delegate.page_size + 1;
            let last = (first + delegate.entries.len()).saturating_sub(1);
            (
                first,
                last,
                delegate.total,
                delegate.expand_all,
                delegate.visible_flags.clone(),
                delegate.failed.clone(),
            )
        };
        let summary: SharedString = match total {
            0 => "No transactions".into(),
            1 => "1 transaction".into(),
            _ if last < first => format!("{total} transactions").into(),
            _ => format!("{first}–{last} of {total} transactions").into(),
        };
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::activate_selected_row))
            .on_action(cx.listener(Self::on_open_account))
            .on_action(cx.listener(Self::on_toggle_postings))
            .on_action(cx.listener(Self::on_edit_entry))
            .on_action(cx.listener(Self::on_show_in_context))
            .on_action(cx.listener(Self::on_show_in_journal))
            .on_action(cx.listener(|this, _: &Search, window, cx| this.focus_search(window, cx)))
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
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .relative()
                                    .w(px(300.))
                                    .max_w_full()
                                    .capture_action(self.search_drag_guard.paste_capture())
                                    .child(self.search_drag_guard.element())
                                    .child(
                                        Input::new(&self.search_input)
                                            .id((ElementId::from("journal-search"), self.page.id()))
                                            .small()
                                            .cleanable(true)
                                            .aria_label("Search entries")
                                            .prefix(
                                                Icon::new(IconName::Search)
                                                    .small()
                                                    .text_color(cx.theme().muted_foreground),
                                            ),
                                    ),
                            )
                            .child(self.render_record_type_selector(&visible_flags, cx)),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_4()
                            .flex_shrink_0()
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
                div().flex_1().min_h_0().px_6().py_3().child(
                    table_card(cx).size_full().child(
                        DataTable::new(&self.table)
                            .small()
                            .bordered(false)
                            .stripe(false),
                    ),
                ),
            )
            .child(self.render_footer(cx))
    }
}

impl JournalDelegate {
    fn new(source: Arc<dyn LedgerSource>, account: Option<SharedString>) -> Self {
        // A posting's cost and price share the Position column in beancount
        // notation (`10 VTI {250.00 USD, 2024-03-01} @ 1.20 EUR`), so the
        // running balance of an account page stays in view.
        let mut columns = vec![
            // Room for the date as a small button.
            Column::new("date", "Date").width(px(120.)).sortable(),
            Column::new("flag", "Flag").width(px(48.)).text_center(),
            Column::new("description", "Description").width(px(if account.is_some() {
                320.
            } else {
                380.
            })),
            Column::new("position", "Position")
                .width(px(280.))
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
            query: SharedString::default(),
            visible_flags: RECORD_TYPES.iter().map(|(flag, _, _)| *flag).collect(),
            focused_entry: None,
            page: 1,
            page_size: DEFAULT_ENTRIES_PER_PAGE,
            total: 0,
            loading: false,
            failed: None,
            generation: 0,
            _load_task: None,
        }
    }

    fn filter_options(&self, page: Option<usize>) -> FilterOptions {
        let query = (!self.query.trim().is_empty()).then(|| self.query.to_string());
        let flags = (self.visible_flags.len() < RECORD_TYPES.len()).then(|| {
            RECORD_TYPES
                .iter()
                .filter(|(flag, _, _)| self.visible_flags.contains(flag))
                .map(|(_, label, _)| *label)
                .collect::<Vec<_>>()
                .join(",")
        });
        FilterOptions {
            entries: Some(self.page_size),
            page,
            old_first: Some(self.old_first),
            query,
            flags,
            ..Default::default()
        }
    }

    /// Reads the current page, replacing the entries shown. The previous
    /// entries stay on screen until the new ones arrive; a page past the end
    /// (after a reload shrank the journal) falls back to the last page.
    fn load_page(&mut self, cx: &mut Context<TableState<Self>>) {
        self.loading = true;
        self.failed = None;
        self.generation += 1;
        let generation = self.generation;
        let options = self.filter_options(Some(self.page));
        let task = self.source.journal(self.account.as_deref(), &options, cx);
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
                            let last_page = total.div_ceil(delegate.page_size).max(1);
                            if entries.is_empty() && total > 0 && delegate.page > last_page {
                                delegate.page = last_page;
                                delegate.load_page(cx);
                                return;
                            }
                            delegate.total = total;
                            delegate.entries = entries;
                            delegate.expanded.clear();
                            delegate.rebuild_rows();
                            sync_focused_selection(table, false, cx);
                        }
                        Err(error) => delegate.failed = Some(format!("{error:#}").into()),
                    }
                    cx.notify();
                })
                .ok();
        }));
        cx.notify();
    }

    /// Reloads entries after a search or flag-filter change. When the user has
    /// a focused record and it still matches the active filters, loads the page
    /// containing that record and scrolls it into view; otherwise falls back to
    /// page 1 while preserving `focused_entry`.
    fn load_filtered_page(&mut self, cx: &mut Context<TableState<Self>>) {
        let Some(target) = self.focused_entry.clone() else {
            self.page = 1;
            self.load_page(cx);
            return;
        };
        self.loading = true;
        self.failed = None;
        self.generation += 1;
        let generation = self.generation;
        let options = self.filter_options(None);
        let page_task = self
            .source
            .journal_page_of(self.account.as_deref(), &options, &target, cx);
        self._load_task = Some(cx.spawn(async move |table, cx| {
            let (page, found) = match page_task.await {
                Ok(Some(page)) => (page, true),
                Ok(None) => (1, false),
                Err(error) => {
                    table
                        .update(cx, |table, cx| {
                            let delegate = table.delegate_mut();
                            if delegate.generation != generation {
                                return;
                            }
                            delegate.loading = false;
                            delegate.failed = Some(format!("{error:#}").into());
                            cx.notify();
                        })
                        .ok();
                    return;
                }
            };
            let Some(journal_task) = table
                .update(cx, |table, cx| {
                    let delegate = table.delegate_mut();
                    if delegate.generation != generation {
                        return None;
                    }
                    delegate.page = page;
                    let options = delegate.filter_options(Some(page));
                    Some(
                        delegate
                            .source
                            .journal(delegate.account.as_deref(), &options, cx),
                    )
                })
                .ok()
                .flatten()
            else {
                return;
            };
            let result = journal_task.await;
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
                            delegate.entries = entries;
                            delegate.expanded.clear();
                            delegate.rebuild_rows();
                            sync_focused_selection(table, found, cx);
                        }
                        Err(error) => delegate.failed = Some(format!("{error:#}").into()),
                    }
                    cx.notify();
                })
                .ok();
        }));
        cx.notify();
    }

    fn focus_entry(&mut self, entry_ix: usize) -> bool {
        let Some(src) = self
            .entries
            .get(entry_ix)
            .map(|entry| entry.txn.src().clone())
        else {
            return false;
        };
        if self.focused_entry.as_ref() == Some(&src) {
            return false;
        }
        self.focused_entry = Some(src);
        true
    }

    fn focus_row(&mut self, row_ix: usize) -> bool {
        match self.rows.get(row_ix).copied() {
            Some(JournalRow::Transaction(entry_ix) | JournalRow::Posting(entry_ix, _)) => {
                self.focus_entry(entry_ix)
            }
            None => false,
        }
    }

    fn is_focused_entry(&self, entry_ix: usize) -> bool {
        self.focused_entry.as_ref().is_some_and(|focused| {
            self.entries
                .get(entry_ix)
                .is_some_and(|entry| entry.txn.src() == focused)
        })
    }

    fn focused_row_ix(&self) -> Option<usize> {
        let target = self.focused_entry.as_ref()?;
        self.rows.iter().position(|row| {
            matches!(row, JournalRow::Transaction(ix)
                if self.entries[*ix].txn.src() == target)
        })
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

    fn toggle_entry(&mut self, entry_ix: usize) {
        if self.expand_all {
            self.expand_all = false;
            self.expanded = (0..self.entries.len())
                .filter(|&ix| ix != entry_ix && self.entries[ix].txn.flag() != TxnFlag::Balance)
                .collect();
        } else if !self.expanded.remove(&entry_ix) {
            self.expanded.insert(entry_ix);
        }
        self.rebuild_rows();
    }
}

fn sync_focused_selection(
    table: &mut TableState<JournalDelegate>,
    scroll: bool,
    cx: &mut Context<TableState<JournalDelegate>>,
) {
    if let Some(row_ix) = table.delegate().focused_row_ix() {
        if table.selected_row() != Some(row_ix) {
            table.set_selected_row(row_ix, cx);
        }
        if scroll {
            table.scroll_to_row(row_ix, cx);
        }
    } else if table.selected_row().is_some() {
        table.clear_selection(cx);
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

fn flag_color(flag: TxnFlag, cx: &App) -> Hsla {
    match flag {
        TxnFlag::Posted => cx.theme().muted_foreground,
        TxnFlag::Pending => cx.theme().yellow,
        TxnFlag::Pad => cx.theme().cyan,
        TxnFlag::Balance => cx.theme().green,
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

    fn render_header(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        table_header()
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
            self.page = 1;
            self.load_page(cx);
        }
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        // Alternates background by transaction index (or highlights the user-focused
        // record) so expanded posting sub-rows share their parent's background color.
        let row = div().id(("row", row_ix));
        match self.rows.get(row_ix).copied() {
            Some(JournalRow::Transaction(entry_ix)) => {
                let bg = if self.is_focused_entry(entry_ix) {
                    table_focused_row_bg(cx)
                } else {
                    table_row_bg(entry_ix, cx)
                };
                row.bg(bg)
            }
            Some(JournalRow::Posting(entry_ix, _)) => {
                let bg = if self.is_focused_entry(entry_ix) {
                    table_focused_row_bg(cx)
                } else {
                    table_row_bg(entry_ix, cx)
                };
                row.bg(bg).text_color(cx.theme().muted_foreground)
            }
            None => row,
        }
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        self.focus_row(row_ix);
        let in_account = self.account.is_some();
        match self.rows.get(row_ix) {
            Some(&JournalRow::Transaction(entry_ix)) => {
                let menu = menu.menu("Edit entry", Box::new(EditEntry(entry_ix)));
                let menu = if self.entries[entry_ix].txn.flag() == TxnFlag::Balance {
                    menu
                } else {
                    let label = if self.is_expanded(entry_ix) {
                        "Hide postings"
                    } else {
                        "Show postings"
                    };
                    menu.menu(label, Box::new(TogglePostings(entry_ix)))
                };
                let menu = menu.menu("Show in context", Box::new(ShowInContext(entry_ix)));
                if in_account {
                    menu.menu("Show in Journal", Box::new(ShowInJournal(entry_ix)))
                } else {
                    menu
                }
            }
            Some(&JournalRow::Posting(entry_ix, posting_ix)) => {
                let account = self.entries[entry_ix].txn.postings()[posting_ix]
                    .account
                    .to_string();
                let menu = menu
                    .menu("Open account", Box::new(OpenAccount(account.into())))
                    .menu("Show in context", Box::new(ShowInContext(entry_ix)));
                if in_account {
                    menu.menu("Show in Journal", Box::new(ShowInJournal(entry_ix)))
                } else {
                    menu
                }
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
                    // The date opens the editor on the transaction.
                    "date" => div()
                        .h_flex()
                        .child(
                            interactive_button(("edit", entry_ix), cx)
                                .font_family(mono)
                                .label(txn.date().to_string())
                                .accessibility_label(format!("Edit the entry of {}", txn.date()))
                                .on_click(cx.listener(move |table, _, window, cx| {
                                    table.delegate_mut().focus_entry(entry_ix);
                                    sync_focused_selection(table, false, cx);
                                    // Along the table's path, whatever has focus, so
                                    // the journal above the table receives it; after
                                    // this update of the table, which it reads.
                                    let handle = table.focus_handle(cx);
                                    window.defer(cx, move |window, cx| {
                                        handle.dispatch_action(&EditEntry(entry_ix), window, cx);
                                    });
                                })),
                        )
                        .into_any_element(),
                    "flag" => div()
                        .font_family(mono)
                        .text_color(flag_color(txn.flag(), cx))
                        .when(txn.flag() == TxnFlag::Pending, |this| this.font_semibold())
                        .child(flag_text(txn.flag()))
                        .into_any_element(),
                    "description" => {
                        if txn.flag() == TxnFlag::Balance {
                            let account: SharedString = txn
                                .postings()
                                .first()
                                .map(|posting| posting.account.to_string())
                                .unwrap_or_default()
                                .into();
                            let to_open = account.clone();
                            div()
                                .h_flex()
                                .child(
                                    interactive_button(("balance-account", entry_ix), cx)
                                        .font_family(mono)
                                        .label(account.clone())
                                        .accessibility_label(format!("Open account {account}"))
                                        .on_click(cx.listener(move |table, _, window, cx| {
                                            table.delegate_mut().focus_entry(entry_ix);
                                            sync_focused_selection(table, false, cx);
                                            let handle = table.focus_handle(cx);
                                            let account = to_open.clone();
                                            window.defer(cx, move |window, cx| {
                                                handle.dispatch_action(
                                                    &OpenAccount(account),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        })),
                                )
                                .into_any_element()
                        } else {
                            let expanded = self.is_expanded(entry_ix);
                            let tag_color = cx.theme().cyan;
                            let link_color = cx.theme().magenta;
                            let cell_pad = Size::Small.table_cell_padding().left;
                            div()
                                .id(("postings", entry_ix))
                                .test_support()
                                .role(Role::Button)
                                .aria_label(if expanded {
                                    "Hide postings"
                                } else {
                                    "Show postings"
                                })
                                .absolute()
                                .inset_0()
                                .px(cell_pad)
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .overflow_hidden()
                                .cursor_pointer()
                                .on_click(cx.listener(move |table, _, _, cx| {
                                    cx.stop_propagation();
                                    let delegate = table.delegate_mut();
                                    delegate.focus_entry(entry_ix);
                                    delegate.toggle_entry(entry_ix);
                                    sync_focused_selection(table, false, cx);
                                    cx.notify();
                                }))
                                .when(!txn.payee().is_empty(), |this| {
                                    this.child(div().font_semibold().child(txn.payee().clone()))
                                })
                                .when(!txn.narration().is_empty(), |this| {
                                    this.child(txn.narration().clone())
                                })
                                .children(txn.tags().iter().map(|tag| {
                                    div()
                                        .font_family(mono.clone())
                                        .text_xs()
                                        .text_color(tag_color)
                                        .child(tag.clone())
                                }))
                                .children(txn.links().iter().map(|link| {
                                    div()
                                        .font_family(mono.clone())
                                        .text_xs()
                                        .text_color(link_color)
                                        .child(link.clone())
                                }))
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
                    "description" => {
                        let account: SharedString = posting.account.to_string().into();
                        let to_open = account.clone();
                        div()
                            .h_flex()
                            .pl_5()
                            .child(
                                interactive_button(("posting-account", row_ix), cx)
                                    .font_family(mono)
                                    .label(account.clone())
                                    .accessibility_label(format!("Open account {account}"))
                                    .on_click(cx.listener(move |table, _, window, cx| {
                                        table.delegate_mut().focus_entry(entry_ix);
                                        sync_focused_selection(table, false, cx);
                                        let handle = table.focus_handle(cx);
                                        let account = to_open.clone();
                                        window.defer(cx, move |window, cx| {
                                            handle.dispatch_action(
                                                &OpenAccount(account),
                                                window,
                                                cx,
                                            );
                                        });
                                    })),
                            )
                            .into_any_element()
                    }
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

    /// The table would replace its rows with a skeleton while loading; the
    /// previous page stays readable instead until the next one arrives.
    fn loading(&self, _: &App) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "journal_report_test.rs"]
mod tests;

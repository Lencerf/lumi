use std::sync::Arc;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState,
};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window,
    div, px,
};
use lumi::web::Position;
use rust_decimal::Decimal;

use crate::journal_report::{ActivateRow, OpenAccount};
use crate::navigation::ReportEvent;
use crate::session::LedgerSession;
use crate::source::{LedgerSource, Positions};
use crate::style::{interactive_button, table_card};

/// The key context of the holdings table; its key bindings apply within it.
pub(crate) const KEY_CONTEXT: &str = "HoldingsReport";

/// Every non-zero position held in an asset or liability account, with its
/// cost basis and book value. A row opens its account on Enter, double
/// click, or from its context menu.
pub struct HoldingsReport {
    session: Entity<LedgerSession>,
    table: Entity<TableState<HoldingsDelegate>>,
    requested_revision: Option<u64>,
    _subscriptions: Vec<Subscription>,
}

/// One position, formatted for its row.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Holding {
    account: SharedString,
    units: SharedString,
    cost: SharedString,
    acquired: SharedString,
    book_value: SharedString,
}

struct HoldingsDelegate {
    source: Arc<dyn LedgerSource>,
    columns: Vec<Column>,
    holdings: Vec<Holding>,
    loading: bool,
    failed: Option<SharedString>,
    generation: u64,
    _load_task: Option<Task<()>>,
}

impl HoldingsReport {
    pub fn new(
        session: Entity<LedgerSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = session.read(cx).source().clone();
        let table = cx.new(|cx| TableState::new(HoldingsDelegate::new(source), window, cx));
        let _subscriptions = vec![
            cx.observe(&session, |this, _, cx| this.load(cx)),
            cx.subscribe(&table, Self::on_table_event),
        ];
        let mut this = Self {
            session,
            table,
            requested_revision: None,
            _subscriptions,
        };
        this.load(cx);
        this
    }

    /// The accounts of the rows shown, in table order.
    pub fn accounts(&self, cx: &App) -> Vec<SharedString> {
        self.table
            .read(cx)
            .delegate()
            .holdings
            .iter()
            .map(|holding| holding.account.clone())
            .collect()
    }

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
            delegate.load(cx);
        });
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.table
            .update(cx, |table, cx| table.delegate_mut().load(cx));
    }

    fn open_row(&mut self, row_ix: usize, cx: &mut Context<Self>) {
        if let Some(holding) = self.table.read(cx).delegate().holdings.get(row_ix) {
            cx.emit(ReportEvent::OpenAccount(holding.account.clone()));
        }
    }

    fn open_selected_row(&mut self, _: &ActivateRow, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(row_ix) = self.table.read(cx).selected_row() {
            self.open_row(row_ix, cx);
        }
    }

    fn on_open_account(&mut self, action: &OpenAccount, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ReportEvent::OpenAccount(action.0.clone()));
    }

    fn on_table_event(
        &mut self,
        _: Entity<TableState<HoldingsDelegate>>,
        event: &TableEvent,
        cx: &mut Context<Self>,
    ) {
        if let TableEvent::DoubleClickedRow(row_ix) = event {
            self.open_row(*row_ix, cx);
        }
    }
}

impl EventEmitter<ReportEvent> for HoldingsReport {}

impl Render for HoldingsReport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (count, failed) = {
            let delegate = self.table.read(cx).delegate();
            (delegate.holdings.len(), delegate.failed.clone())
        };
        let summary: SharedString = if count == 1 {
            "1 position".into()
        } else {
            format!("{count} positions").into()
        };
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::open_selected_row))
            .on_action(cx.listener(Self::on_open_account))
            .child(
                div()
                    .px_6()
                    .pt_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary),
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
                            Alert::error("holdings-failed", message)
                                .title("Couldn't read the holdings"),
                        )
                        .child(
                            Button::new("holdings-retry")
                                .label("Retry")
                                .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                        ),
                )
            })
            .child(
                div().flex_1().min_h_0().p_6().pt_3().child(
                    table_card(cx).size_full().child(
                        DataTable::new(&self.table)
                            .small()
                            .bordered(false)
                            .stripe(true),
                    ),
                ),
            )
    }
}

impl HoldingsDelegate {
    fn new(source: Arc<dyn LedgerSource>) -> Self {
        Self {
            source,
            columns: vec![
                Column::new("account", "Account").width(px(320.)).sortable(),
                Column::new("units", "Units").width(px(170.)).text_right(),
                Column::new("cost", "Cost").width(px(150.)).text_right(),
                Column::new("acquired", "Acquired")
                    .width(px(120.))
                    .text_right(),
                Column::new("book_value", "Book value")
                    .width(px(170.))
                    .text_right(),
            ],
            holdings: Vec::new(),
            loading: false,
            failed: None,
            generation: 0,
            _load_task: None,
        }
    }

    fn load(&mut self, cx: &mut Context<TableState<Self>>) {
        self.loading = true;
        self.failed = None;
        self.generation += 1;
        let generation = self.generation;
        let task = self.source.positions(cx);
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
                        Ok(positions) => delegate.holdings = holdings(positions),
                        Err(error) => delegate.failed = Some(format!("{error:#}").into()),
                    }
                    cx.notify();
                })
                .ok();
        }));
        cx.notify();
    }
}

/// The rows for `positions`: non-zero positions of asset and liability
/// accounts, by account, then currency, acquisition date and cost.
fn holdings(positions: Positions) -> Vec<Holding> {
    let mut accounts: Vec<_> = positions
        .into_iter()
        .filter(|(account, _)| account.starts_with("Assets") || account.starts_with("Liabilities"))
        .collect();
    accounts.sort_by(|a, b| a.0.cmp(&b.0));
    let mut rows = Vec::new();
    for (account, mut positions) in accounts {
        positions.sort_by(|a, b| {
            let key = |position: &Position| {
                (
                    position.currency.clone(),
                    position.cost.as_ref().map(|cost| cost.date),
                    position
                        .cost
                        .as_ref()
                        .map_or(Decimal::ZERO, |cost| cost.amount.number),
                )
            };
            key(a).cmp(&key(b))
        });
        let account: SharedString = account.into();
        for position in positions {
            if position.number.is_zero() {
                continue;
            }
            let units: SharedString = format!("{} {}", position.number, position.currency).into();
            let holding = match &position.cost {
                Some(cost) => Holding {
                    account: account.clone(),
                    units,
                    cost: cost.amount.to_string().into(),
                    acquired: cost.date.to_string().into(),
                    book_value: format!(
                        "{} {}",
                        position.number * cost.amount.number,
                        cost.amount.currency
                    )
                    .into(),
                },
                None => Holding {
                    account: account.clone(),
                    book_value: units.clone(),
                    units,
                    cost: SharedString::default(),
                    acquired: SharedString::default(),
                },
            };
            rows.push(holding);
        }
    }
    rows
}

impl TableDelegate for HoldingsDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.holdings.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        if self.columns[col_ix].key.as_ref() != "account" {
            return;
        }
        match sort {
            ColumnSort::Descending => self.holdings.sort_by(|a, b| b.account.cmp(&a.account)),
            _ => self.holdings.sort_by(|a, b| a.account.cmp(&b.account)),
        }
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        match self.holdings.get(row_ix) {
            Some(holding) => menu.menu(
                "Open account",
                Box::new(OpenAccount(holding.account.clone())),
            ),
            None => menu,
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let holding = &self.holdings[row_ix];
        let mono = cx.theme().mono_font_family.clone();
        // Every column is an identifier or a number, so all are monospaced.
        match self.columns[col_ix].key.as_ref() {
            "account" => {
                let account = holding.account.clone();
                let to_open = account.clone();
                div()
                    .h_flex()
                    .child(
                        interactive_button(("holding-account", row_ix), cx)
                            .font_family(mono)
                            .label(account.clone())
                            .accessibility_label(format!("Open account {account}"))
                            .on_click(cx.listener(move |table, _, window, cx| {
                                let handle = table.focus_handle(cx);
                                let account = to_open.clone();
                                window.defer(cx, move |window, cx| {
                                    handle.dispatch_action(&OpenAccount(account), window, cx);
                                });
                            })),
                    )
                    .into_any_element()
            }
            "units" => div()
                .font_family(mono)
                .child(holding.units.clone())
                .into_any_element(),
            "cost" => div()
                .font_family(mono)
                .child(holding.cost.clone())
                .into_any_element(),
            "acquired" => div()
                .font_family(mono)
                .child(holding.acquired.clone())
                .into_any_element(),
            "book_value" => div()
                .font_family(mono)
                .child(holding.book_value.clone())
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, _: &App) -> Option<SharedString> {
        let holding = self.holdings.get(row_ix)?;
        Some(match self.columns.get(col_ix)?.key.as_ref() {
            "account" => holding.account.clone(),
            "units" => holding.units.clone(),
            "cost" => holding.cost.clone(),
            "acquired" => holding.acquired.clone(),
            "book_value" => holding.book_value.clone(),
            _ => SharedString::default(),
        })
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
            .child("No positions")
    }

    fn loading(&self, _: &App) -> bool {
        self.loading
    }
}

#[cfg(test)]
#[path = "holdings_report_test.rs"]
mod tests;

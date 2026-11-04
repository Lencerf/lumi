use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::Button;
use gpui_kit::component::calendar::Date;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::date_picker::{DatePickerEvent, DatePickerState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Size, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, EventEmitter, Font, FontWeight,
    Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    Styled as _, Subscription, Task, TestSupportExt as _, Window, div, px, relative, rems,
};
use lumi::web::{
    BalanceSheetSeries, ChartDatePreset, ChartDateRange, IncomeExpenseSeries, NaiveDate,
    TimeInterval, TrieOptions, TrieTable,
};

#[cfg(not(target_family = "wasm"))]
use crate::chart::MIN_CHART_INNER_WIDTH;
use crate::chart::{
    BalanceSheetMode, ChartDateFilter, CurrencySelection, IncomeChartMode, new_chart_date_picker,
    render_balance_sheet_chart, render_date_range_controls, render_income_expense_chart,
    sync_chart_date_picker,
};
#[cfg(test)]
use crate::chart::{balance_sheet_mode_currencies, income_mode_currencies};
use crate::navigation::{Page, ReportEvent};
use crate::number::{GroupedText, shape_text_width};
use crate::session::LedgerSession;
use crate::style::{
    available_report_content_width, horizontal_scroll_area, interactive_button,
    scale_widths_proportionally, table_card, table_row_bg, trie_cell_col_separator,
    trie_head_col_separator,
};

/// A report made of account hierarchies with their balances, such as the
/// balance sheet (Assets beside Liabilities and Equity) or the income
/// statement (Income beside Expenses).
///
/// The report owns its load state. It queries the session's source whenever
/// the ledger revision or its own options change, keeps the previous tables
/// on screen while a reload is in flight, and ignores results that arrive for
/// a request it has since superseded.
pub struct TrieReport {
    page: Page,
    session: Entity<LedgerSession>,
    show_closed: bool,
    tables: Vec<TrieLoad>,
    /// What `tables` was requested for; `None` before the first request.
    requested: Option<LoadKey>,
    /// Counts requests so that a late result for an earlier one is dropped.
    generation: u64,
    _load_task: Option<Task<()>>,
    chart: ReportChart,
    chart_currencies: CurrencySelection,
    chart_date_filter: ChartDateFilter,
    date_picker: Option<Entity<DatePickerState>>,
    balance_sheet_mode: BalanceSheetMode,
    income_chart_mode: IncomeChartMode,
    income_interval: TimeInterval,
    chart_requested: Option<ChartLoadKey>,
    chart_generation: u64,
    _chart_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// The ledger state and options a set of tables reflects.
#[derive(Clone, Copy, PartialEq, Eq)]
struct LoadKey {
    revision: u64,
    show_closed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ChartLoadKey {
    revision: u64,
    interval: TimeInterval,
    range: ChartDateRange,
}

enum ReportChart {
    None,
    BalanceSheet(BalanceSheetSeries),
    Income(IncomeExpenseSeries),
}

enum TrieLoad {
    Pending,
    Ready(TriePresentation),
    /// No account starts with the root.
    Missing,
    Failed(SharedString),
}

/// One column of the tree prefix drawn in front of an account component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeGuide {
    /// `│   `: an ancestor level that still has siblings below this row.
    Pass,
    /// `    `: an ancestor level whose branch already ended above this row.
    Space,
    /// `├── `: connects this row to its parent when more siblings follow.
    Branch,
    /// `└── `: connects this row to its parent as the final sibling.
    Last,
}

/// One account hierarchy prepared for rendering.
struct TriePresentation {
    currencies: Vec<SharedString>,
    rows: Vec<TrieRow>,
}

struct TrieRow {
    /// The full account name, e.g. `Assets:Bank:Checking`.
    account: SharedString,
    level: usize,
    /// Tree guide segments (`level` columns) drawn before `name`.
    guides: Vec<TreeGuide>,
    /// The last segment of the account name.
    name: SharedString,
    /// One formatted balance per currency; empty for a zero balance.
    numbers: Vec<GroupedText>,
}

#[cfg(test)]
impl TrieRow {
    pub(crate) fn tree_line(&self) -> String {
        let mut line = String::new();
        for guide in &self.guides {
            line.push_str(match guide {
                TreeGuide::Pass => "│   ",
                TreeGuide::Space => "    ",
                TreeGuide::Branch => "├── ",
                TreeGuide::Last => "└── ",
            });
        }
        line.push_str(&self.name);
        line
    }
}

/// The root accounts of each column of `page`.
fn columns(page: &Page) -> &'static [&'static [&'static str]] {
    match page {
        Page::Income => &[&["Income"], &["Expenses"]],
        _ => &[&["Assets"], &["Liabilities", "Equity"]],
    }
}

fn roots(page: &Page) -> impl Iterator<Item = &'static str> {
    columns(page)
        .iter()
        .flat_map(|column| column.iter().copied())
}

impl TrieReport {
    pub fn new(page: Page, session: Entity<LedgerSession>, cx: &mut Context<Self>) -> Self {
        let _subscriptions = vec![cx.observe(&session, |this, _, cx| this.load(cx))];
        let mut this = Self {
            page,
            session,
            show_closed: false,
            tables: Vec::new(),
            requested: None,
            generation: 0,
            _load_task: None,
            chart: ReportChart::None,
            chart_currencies: CurrencySelection::default(),
            chart_date_filter: ChartDateFilter::default(),
            date_picker: None,
            balance_sheet_mode: BalanceSheetMode::default(),
            income_chart_mode: IncomeChartMode::default(),
            income_interval: TimeInterval::default(),
            chart_requested: None,
            chart_generation: 0,
            _chart_task: None,
            _subscriptions,
        };
        this.load(cx);
        this
    }

    pub fn page(&self) -> &Page {
        &self.page
    }

    pub fn is_showing_closed(&self) -> bool {
        self.show_closed
    }

    pub fn balance_sheet_mode(&self) -> BalanceSheetMode {
        self.balance_sheet_mode
    }

    pub fn income_chart_mode(&self) -> IncomeChartMode {
        self.income_chart_mode
    }

    pub fn income_interval(&self) -> TimeInterval {
        self.income_interval
    }

    pub fn chart_date_preset(&self) -> ChartDatePreset {
        self.chart_date_filter.preset()
    }

    pub fn chart_date_range(&self) -> ChartDateRange {
        self.chart_date_filter.effective_range()
    }

    #[cfg(test)]
    pub(crate) fn chart_date_picker(&self) -> Option<&Entity<DatePickerState>> {
        self.date_picker.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn set_chart_today(&mut self, today: NaiveDate, cx: &mut Context<Self>) {
        if self.chart_date_filter.set_today(today) {
            self.load_chart(cx);
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn selected_chart_currencies(&self) -> Vec<&str> {
        match &self.chart {
            ReportChart::None => Vec::new(),
            ReportChart::BalanceSheet(series) => {
                balance_sheet_mode_currencies(series, self.balance_sheet_mode)
                    .into_iter()
                    .map(|(_, c)| c)
                    .filter(|c| self.chart_currencies.is_selected(c))
                    .collect()
            }
            ReportChart::Income(series) => income_mode_currencies(series, self.income_chart_mode)
                .into_iter()
                .map(|(_, c)| c)
                .filter(|c| self.chart_currencies.is_selected(c))
                .collect(),
        }
    }

    /// The account names of every row currently shown, in order.
    pub fn accounts(&self) -> Vec<SharedString> {
        self.tables
            .iter()
            .filter_map(|table| match table {
                TrieLoad::Ready(presentation) => Some(presentation),
                _ => None,
            })
            .flat_map(|presentation| presentation.rows.iter().map(|row| row.account.clone()))
            .collect()
    }

    fn set_show_closed(&mut self, show_closed: bool, cx: &mut Context<Self>) {
        if self.show_closed != show_closed {
            self.show_closed = show_closed;
            self.load(cx);
            cx.notify();
        }
    }

    fn set_balance_sheet_mode(&mut self, mode: BalanceSheetMode, cx: &mut Context<Self>) {
        if self.balance_sheet_mode != mode {
            self.balance_sheet_mode = mode;
            cx.notify();
        }
    }

    fn set_income_chart_mode(&mut self, mode: IncomeChartMode, cx: &mut Context<Self>) {
        if self.income_chart_mode != mode {
            self.income_chart_mode = mode;
            cx.notify();
        }
    }

    fn set_income_interval(&mut self, interval: TimeInterval, cx: &mut Context<Self>) {
        if self.income_interval != interval {
            self.income_interval = interval;
            self.load_chart(cx);
            cx.notify();
        }
    }

    fn set_currency_selected(
        &mut self,
        currency: SharedString,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if self.chart_currencies.set_selected(currency, selected) {
            cx.notify();
        }
    }

    fn ensure_date_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.date_picker.is_none() {
            let picker = cx.new(|cx| new_chart_date_picker(window, cx));
            sync_chart_date_picker(&picker, &self.chart_date_filter, window, cx);
            self._subscriptions
                .push(cx.subscribe(&picker, Self::on_date_picker_event));
            self.date_picker = Some(picker);
        }
    }

    fn chart_series_bounds(&self) -> Option<(NaiveDate, NaiveDate)> {
        match &self.chart {
            ReportChart::None => None,
            ReportChart::BalanceSheet(series) => series.date_bounds(),
            ReportChart::Income(series) => series.date_bounds(),
        }
    }

    fn select_chart_date_preset(
        &mut self,
        preset: ChartDatePreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.chart_series_bounds();
        if self.chart_date_filter.select_preset(preset, bounds) {
            if preset == ChartDatePreset::Custom
                && let Some(picker) = &self.date_picker
            {
                sync_chart_date_picker(picker, &self.chart_date_filter, window, cx);
            }
            self.load_chart(cx);
            cx.notify();
        }
    }

    fn on_date_picker_event(
        &mut self,
        _: Entity<DatePickerState>,
        event: &DatePickerEvent,
        cx: &mut Context<Self>,
    ) {
        let DatePickerEvent::Change(dt) = event;
        if let Date::Range(start, end) = dt.date()
            && self.chart_date_filter.set_custom_range(start, end)
        {
            self.load_chart(cx);
            cx.notify();
        }
    }

    /// Queries the tables and chart for the current ledger revision and options,
    /// unless those exact requests have already been made.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.load_chart(cx);
        let (revision, source) = {
            let session = self.session.read(cx);
            if !session.is_loaded() {
                return;
            }
            (session.revision(), session.source().clone())
        };
        let key = LoadKey {
            revision,
            show_closed: self.show_closed,
        };
        if self.requested == Some(key) {
            return;
        }
        let options = TrieOptions {
            show_closed: Some(self.show_closed),
        };
        let requests: Vec<_> = roots(&self.page)
            .map(|root| (root, source.trie_table(root, &options, cx)))
            .collect();
        if self.tables.is_empty() {
            self.tables = requests.iter().map(|_| TrieLoad::Pending).collect();
        }
        self.requested = Some(key);
        self.generation += 1;
        let generation = self.generation;
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let mut results = Vec::with_capacity(requests.len());
            for (root, task) in requests {
                results.push((root, task.await));
            }
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.tables = results
                    .into_iter()
                    .map(|(root, result)| match result {
                        Ok(Some(table)) => TrieLoad::Ready(TriePresentation::new(root, table)),
                        Ok(None) => TrieLoad::Missing,
                        Err(error) => TrieLoad::Failed(format!("{error:#}").into()),
                    })
                    .collect();
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn load_chart(&mut self, cx: &mut Context<Self>) {
        let (revision, source) = {
            let session = self.session.read(cx);
            if !session.is_loaded() {
                return;
            }
            (session.revision(), session.source().clone())
        };
        let key = match self.page {
            Page::Income => ChartLoadKey {
                revision,
                interval: self.income_interval,
                range: self.chart_date_filter.effective_range(),
            },
            _ => ChartLoadKey {
                revision,
                interval: TimeInterval::default(),
                range: ChartDateRange::ALL,
            },
        };
        if self.chart_requested == Some(key) {
            return;
        }
        self.chart_requested = Some(key);
        self.chart_generation += 1;
        let generation = self.chart_generation;
        match self.page {
            Page::Income => {
                let task = source.income_expense_series(key.interval, key.range, cx);
                self._chart_task = Some(cx.spawn(async move |this, cx| {
                    let Ok(series) = task.await else {
                        return;
                    };
                    this.update(cx, |this, cx| {
                        if this.chart_generation != generation {
                            return;
                        }
                        this.chart = ReportChart::Income(series);
                        cx.notify();
                    })
                    .ok();
                }));
            }
            _ => {
                let task = source.balance_sheet_series(cx);
                self._chart_task = Some(cx.spawn(async move |this, cx| {
                    let Ok(series) = task.await else {
                        return;
                    };
                    this.update(cx, |this, cx| {
                        if this.chart_generation != generation {
                            return;
                        }
                        this.chart = ReportChart::BalanceSheet(series);
                        cx.notify();
                    })
                    .ok();
                }));
            }
        }
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.requested = None;
        self.chart_requested = None;
        self.load(cx);
    }

    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn preferred_content_width(&self, window: &Window, cx: &App) -> Option<Pixels> {
        if self.tables.is_empty()
            || self
                .tables
                .iter()
                .any(|table| matches!(table, TrieLoad::Pending))
        {
            return None;
        }
        let col_naturals = self.measure_column_card_widths(window, cx);
        let gap = rems(1.5).to_pixels(window.rem_size());
        let total_gaps = gap * col_naturals.len().saturating_sub(1) as f32;
        let pair_width = col_naturals.into_iter().sum::<Pixels>() + total_gaps;
        Some(pair_width.max(MIN_CHART_INNER_WIDTH + px(2.)))
    }

    fn measure_column_card_widths(&self, window: &Window, cx: &App) -> Vec<Pixels> {
        let mut iter = self.tables.iter();
        columns(&self.page)
            .iter()
            .map(|column| {
                column
                    .iter()
                    .map(|_| match iter.next().unwrap_or(&TrieLoad::Pending) {
                        TrieLoad::Ready(presentation) => {
                            let (acct, ccys) = measure_table_columns(presentation, window, cx);
                            acct + ccys.into_iter().sum::<Pixels>() + px(2.)
                        }
                        TrieLoad::Failed(_) => px(280.),
                        TrieLoad::Pending | TrieLoad::Missing => px(180.),
                    })
                    .fold(px(180.), Pixels::max)
            })
            .collect()
    }

    fn render_chart(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let preset_entity = cx.entity().downgrade();
        let date_controls = render_date_range_controls(
            self.page.id(),
            &self.chart_date_filter,
            self.date_picker.as_ref(),
            move |preset, window, cx| {
                let _ = preset_entity.update(cx, |this, cx| {
                    this.select_chart_date_preset(preset, window, cx);
                });
            },
        );

        match &self.chart {
            ReportChart::None => None,
            ReportChart::BalanceSheet(series) => {
                let mode_entity = cx.entity().downgrade();
                let currency_entity = cx.entity().downgrade();
                render_balance_sheet_chart(
                    (series, self.balance_sheet_mode),
                    (&self.chart_currencies, &self.chart_date_filter),
                    date_controls,
                    move |mode, _, cx| {
                        let _ = mode_entity.update(cx, |this, cx| {
                            this.set_balance_sheet_mode(mode, cx);
                        });
                    },
                    move |currency, selected, _, cx| {
                        let _ = currency_entity.update(cx, |this, cx| {
                            this.set_currency_selected(currency, selected, cx);
                        });
                    },
                    window,
                    cx,
                )
            }
            ReportChart::Income(series) => {
                let mode_entity = cx.entity().downgrade();
                let interval_entity = cx.entity().downgrade();
                let currency_entity = cx.entity().downgrade();
                render_income_expense_chart(
                    series,
                    (self.income_chart_mode, self.income_interval),
                    (&self.chart_currencies, date_controls),
                    (
                        move |mode, _, cx| {
                            let _ = mode_entity.update(cx, |this, cx| {
                                this.set_income_chart_mode(mode, cx);
                            });
                        },
                        move |interval, _, cx| {
                            let _ = interval_entity.update(cx, |this, cx| {
                                this.set_income_interval(interval, cx);
                            });
                        },
                    ),
                    move |currency, selected, _, cx| {
                        let _ = currency_entity.update(cx, |this, cx| {
                            this.set_currency_selected(currency, selected, cx);
                        });
                    },
                    window,
                    cx,
                )
            }
        }
    }

    fn open_account(&mut self, account: SharedString, cx: &mut Context<Self>) {
        cx.emit(ReportEvent::OpenAccount(account));
    }

    fn render_table(
        &self,
        root: &'static str,
        table: &TrieLoad,
        target_card_width: Option<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match table {
            TrieLoad::Pending => div()
                .h_flex()
                .gap_2()
                .text_color(muted)
                .child(Spinner::new().small())
                .child(root)
                .into_any_element(),
            TrieLoad::Missing => div()
                .v_flex()
                .gap_1()
                .child(div().font_semibold().child(root))
                .child(div().text_sm().text_color(muted).child("No accounts"))
                .into_any_element(),
            TrieLoad::Failed(message) => div()
                .v_flex()
                .gap_2()
                .items_start()
                .child(
                    Alert::error((ElementId::from("trie-failed"), root), message.clone())
                        .title(root),
                )
                .child(
                    Button::new((ElementId::from("retry"), root))
                        .label("Retry")
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                )
                .into_any_element(),
            TrieLoad::Ready(presentation) => {
                let (account_min_width, currency_widths) =
                    measure_table_columns(presentation, window, cx);
                let mut col_widths = Vec::with_capacity(1 + currency_widths.len());
                col_widths.push(account_min_width);
                col_widths.extend(currency_widths);
                if let Some(target_card) = target_card_width {
                    col_widths = scale_widths_proportionally(
                        &col_widths,
                        (target_card - px(2.)).max(px(0.)),
                    );
                }
                let account_width = col_widths[0];
                let currency_widths = &col_widths[1..];
                let card_width = col_widths.iter().copied().sum::<Pixels>() + px(2.);
                let theme = cx.theme();
                let mono = theme.mono_font_family.clone();
                let border = theme.border;
                let guide_color = muted.opacity(0.55);
                table_card(cx)
                    .w(card_width)
                    .flex_shrink_0()
                    .child(
                        Table::new()
                            .small()
                            .accessibility_label(root)
                            .child(
                                TableHeader::new().font_medium().child(
                                    TableRow::new()
                                        .child(
                                            TableHead::new()
                                                .w(account_width)
                                                .min_w(account_width)
                                                .flex_shrink_0()
                                                .whitespace_nowrap()
                                                .child("Account"),
                                        )
                                        .children(presentation.currencies.iter().enumerate().map(
                                            |(col_ix, currency)| {
                                                let col_width = currency_widths[col_ix];
                                                TableHead::new()
                                                    .relative()
                                                    .w(col_width)
                                                    .min_w(col_width)
                                                    .flex_shrink_0()
                                                    .whitespace_nowrap()
                                                    .text_right()
                                                    .child(currency.clone())
                                                    .child(trie_head_col_separator(
                                                        root,
                                                        col_ix + 1,
                                                        cx,
                                                    ))
                                            },
                                        )),
                                ),
                            )
                            .child(TableBody::new().border_t_1().border_color(border).children(
                                presentation.rows.iter().enumerate().map(|(row_ix, row)| {
                                    let account = row.account.clone();
                                    let row_bg = table_row_bg(row_ix, cx);
                                    TableRow::new()
                                        .bg(row_bg)
                                        .child(
                                            TableCell::new()
                                                .w(account_width)
                                                .min_w(account_width)
                                                .flex_shrink_0()
                                                .py_0()
                                                .items_stretch()
                                                .children(
                                                    row.guides.iter().copied().enumerate().map(
                                                        |(col_ix, guide)| {
                                                            render_tree_guide(
                                                                &row.account,
                                                                col_ix,
                                                                guide,
                                                                guide_color,
                                                            )
                                                        },
                                                    ),
                                                )
                                                .child(
                                                    div()
                                                        .py(px(3.))
                                                        .flex()
                                                        .items_center()
                                                        .flex_shrink_0()
                                                        .child(
                                                            // Opens the account's journal. Small,
                                                            // like the table, so the name is set
                                                            // like the numbers beside it.
                                                            interactive_button(
                                                                (
                                                                    ElementId::from("account"),
                                                                    row.account.clone(),
                                                                ),
                                                                cx,
                                                            )
                                                            .font_family(mono.clone())
                                                            .when(row.level == 0, |this| {
                                                                this.font_semibold()
                                                            })
                                                            .label(row.name.clone())
                                                            .accessibility_label(
                                                                row.account.clone(),
                                                            )
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    this.open_account(
                                                                        account.clone(),
                                                                        cx,
                                                                    )
                                                                },
                                                            )),
                                                        ),
                                                ),
                                        )
                                        .children(row.numbers.iter().enumerate().map(
                                            |(col_ix, number)| {
                                                let col_width = currency_widths[col_ix];
                                                TableCell::new()
                                                    .relative()
                                                    .w(col_width)
                                                    .min_w(col_width)
                                                    .flex_shrink_0()
                                                    .whitespace_nowrap()
                                                    .text_right()
                                                    .font_family(mono.clone())
                                                    .when(row.level == 0, |this| {
                                                        this.font_semibold()
                                                    })
                                                    .child(number.render())
                                                    .child(trie_cell_col_separator(
                                                        root,
                                                        row_ix,
                                                        col_ix + 1,
                                                        cx,
                                                    ))
                                            },
                                        ))
                                }),
                            )),
                    )
                    .into_any_element()
            }
        }
    }
}

fn render_tree_guide(
    account: &SharedString,
    col_ix: usize,
    guide: TreeGuide,
    line_color: Hsla,
) -> impl IntoElement {
    let id = (
        ElementId::from("tree-guide"),
        SharedString::from(format!("{account}:{col_ix}")),
    );
    let cell = div()
        .id(id)
        .test_support()
        .relative()
        .w(rems(1.25))
        .flex_shrink_0();
    match guide {
        TreeGuide::Space => cell,
        TreeGuide::Pass => cell.child(
            div()
                .absolute()
                .left(px(10.))
                .top(px(-1.))
                .bottom_0()
                .w(px(1.))
                .bg(line_color),
        ),
        TreeGuide::Branch => cell
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(-1.))
                    .bottom_0()
                    .w(px(1.))
                    .bg(line_color),
            )
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .right_0()
                    .top(relative(0.5))
                    .h(px(1.))
                    .bg(line_color),
            ),
        TreeGuide::Last => cell
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(-1.))
                    .bottom(relative(0.5))
                    .w(px(1.))
                    .bg(line_color),
            )
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .right_0()
                    .top(relative(0.5))
                    .h(px(1.))
                    .bg(line_color),
            ),
    }
}

impl TriePresentation {
    /// Flattens the rows of `table`, resolving each row's full account name
    /// and `tree`-style connector guides from the hierarchy.
    fn new(root: &str, table: TrieTable<String>) -> Self {
        let raw_rows = table.rows;
        let mut ancestors: Vec<(String, usize, bool)> = Vec::new();
        let rows: Vec<TrieRow> = raw_rows
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                while ancestors
                    .last()
                    .is_some_and(|(_, level, _)| *level >= row.level)
                {
                    ancestors.pop();
                }
                let has_next_sibling = row.level > 0
                    && raw_rows[ix + 1..]
                        .iter()
                        .find(|next| next.level <= row.level)
                        .is_some_and(|next| next.level == row.level);
                let mut guides = Vec::with_capacity(row.level);
                if row.level > 0 {
                    for (_, _, ancestor_has_next) in ancestors.iter().skip(1) {
                        guides.push(if *ancestor_has_next {
                            TreeGuide::Pass
                        } else {
                            TreeGuide::Space
                        });
                    }
                    guides.push(if has_next_sibling {
                        TreeGuide::Branch
                    } else {
                        TreeGuide::Last
                    });
                }
                ancestors.push((row.name.clone(), row.level, has_next_sibling));
                let account = ancestors
                    .iter()
                    .map(|(name, _, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(":");
                TrieRow {
                    account: account.into(),
                    level: row.level,
                    guides,
                    name: (&row.name).into(),
                    numbers: row
                        .numbers
                        .iter()
                        .map(|n| GroupedText::from_number_str(n))
                        .collect(),
                }
            })
            .collect();
        debug_assert!(
            rows.first().is_none_or(|row| row.account.as_ref() == root),
            "the first row of a trie table is its root"
        );
        Self {
            currencies: table
                .currencies
                .into_iter()
                .map(SharedString::from)
                .collect(),
            rows,
        }
    }
}

/// Measures the `Account` minimum width and each currency column's exact width
/// against the active window's fonts so `TableRow` flex layout neither shrinks
/// currency columns nor clips wide grouped balances.
fn measure_table_columns(
    presentation: &TriePresentation,
    window: &Window,
    cx: &App,
) -> (Pixels, Vec<Pixels>) {
    const MIN_CURRENCY_COL_WIDTH: Pixels = px(100.);
    const CURRENCY_LEFT_BREATHING: Pixels = px(14.);
    const ACCOUNT_BUTTON_EXTRA: Pixels = px(20.);

    let rem = window.rem_size();
    let font_size = rems(0.875).to_pixels(rem);
    let guide_width = rems(1.25).to_pixels(rem);
    let min_account_width = rems(10.).to_pixels(rem);
    let pad = Size::Small.table_cell_padding();
    let cell_h_pad = pad.left + pad.right;

    let theme = cx.theme();
    let head_font = Font {
        family: theme.font_family.clone(),
        weight: FontWeight::MEDIUM,
        ..Default::default()
    };
    let mono_regular = Font {
        family: theme.mono_font_family.clone(),
        weight: FontWeight::NORMAL,
        ..Default::default()
    };
    let mono_semibold = Font {
        family: theme.mono_font_family.clone(),
        weight: FontWeight::SEMIBOLD,
        ..Default::default()
    };

    let text_system = window.text_system();
    let mut account_width =
        shape_text_width(text_system, "Account".into(), head_font.clone(), font_size) + cell_h_pad;
    for row in &presentation.rows {
        let font = if row.level == 0 {
            mono_semibold.clone()
        } else {
            mono_regular.clone()
        };
        let row_width = guide_width * row.level as f32
            + shape_text_width(text_system, row.name.clone(), font, font_size)
            + cell_h_pad
            + ACCOUNT_BUTTON_EXTRA;
        account_width = account_width.max(row_width);
    }
    let account_min_width = px(f32::from(account_width.max(min_account_width)).ceil());

    let currency_extra = cell_h_pad + CURRENCY_LEFT_BREATHING;
    let currency_widths = presentation
        .currencies
        .iter()
        .enumerate()
        .map(|(col_ix, currency)| {
            let mut max_text =
                shape_text_width(text_system, currency.clone(), head_font.clone(), font_size);
            for row in &presentation.rows {
                if let Some(number) = row.numbers.get(col_ix) {
                    let font = if row.level == 0 {
                        mono_semibold.clone()
                    } else {
                        mono_regular.clone()
                    };
                    max_text = max_text.max(number.measure_width(font, font_size, window));
                }
            }
            let total = (max_text + currency_extra).max(MIN_CURRENCY_COL_WIDTH);
            px(f32::from(total).ceil())
        })
        .collect();

    (account_min_width, currency_widths)
}

impl EventEmitter<ReportEvent> for TrieReport {}

impl Render for TrieReport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_date_picker(window, cx);
        let page = self.page.clone();
        let chart = self.render_chart(window, cx);
        let col_naturals = self.measure_column_card_widths(window, cx);
        let gap = rems(1.5).to_pixels(window.rem_size());
        let total_gaps = gap * col_naturals.len().saturating_sub(1) as f32;
        let natural_pair_width = col_naturals.iter().copied().sum::<Pixels>() + total_gaps;
        let available = available_report_content_width(window);
        let col_targets: Option<Vec<Pixels>> = (available > natural_pair_width)
            .then(|| scale_widths_proportionally(&col_naturals, available - total_gaps));

        let mut tables = self.tables.iter();
        let tables_row =
            div()
                .h_flex()
                .items_start()
                .gap_6()
                .children(columns(&page).iter().enumerate().map(|(col_ix, column)| {
                    let target_card_width = col_targets
                        .as_ref()
                        .and_then(|targets| targets.get(col_ix).copied());
                    div()
                        .v_flex()
                        .flex_none()
                        .gap_6()
                        .children(column.iter().map(|root| {
                            let table = tables.next().unwrap_or(&TrieLoad::Pending);
                            // `Table` has no id of its own, so without this scope
                            // sibling tables would share element paths and so
                            // accessibility node ids.
                            div()
                                .id((ElementId::from("trie-table"), *root))
                                .test_support()
                                .child(self.render_table(
                                    root,
                                    table,
                                    target_card_width,
                                    window,
                                    cx,
                                ))
                        }))
                }));
        let tables_scroll = horizontal_scroll_area(
            (ElementId::from("trie-tables-scroll"), page.id()),
            tables_row,
            window,
            cx,
        );

        div()
            .v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(
                div().h_flex().justify_end().px_6().pt_4().child(
                    Checkbox::new((ElementId::from("show-closed"), page.id()))
                        .label("Show closed accounts")
                        .checked(self.show_closed)
                        .on_change(
                            cx.listener(|this, checked, _, cx| this.set_show_closed(*checked, cx)),
                        ),
                ),
            )
            .child(
                // The report scrolls vertically at the page level while charts
                // and hierarchy tables each scroll horizontally at their own
                // level when narrowed, so neither clips nor stretches the other.
                div()
                    .id((ElementId::from("trie-report"), page.id()))
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_6()
                            .p_6()
                            .when_some(chart, |this, chart| this.child(chart))
                            .child(tables_scroll),
                    ),
            )
    }
}

#[cfg(test)]
#[path = "trie_report_test.rs"]
mod tests;

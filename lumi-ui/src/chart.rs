//! Interactive Fava-style charts for the balance sheet, account journal, and
//! income statement, with per-currency checklist filtering.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonGroup};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::plot::label::{TEXT_SIZE, Text, measure_text_width};
use gpui_kit::component::plot::scale::{Scale as _, ScaleLinear};
use gpui_kit::component::plot::shape::{Bar, BarAlignment, Line};
use gpui_kit::component::plot::tooltip::{CrossLine, Dot, Tooltip};
use gpui_kit::component::plot::{
    AxisText, Curve, Grid, PathCaches, Plot, PlotAxis, PlotElement, PlotHover, PlotLabel,
    TooltipState, axis_gutter,
};
use gpui_kit::component::{ActiveTheme as _, Selectable as _, Sizable as _, StyledExt as _, Theme};
use gpui_kit::{
    AnyElement, App, Bounds, Corners, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Point, SharedString, Size, Styled as _, TestSupportExt as _,
    TextAlign, Window, div, point, px,
};
use lumi::web::{
    AccountBalanceSeries, BalanceSheetSeries, IncomeExpenseSeries, TimeInterval, format_day_index,
};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::number::GroupedBuilder;
use crate::style::table_card;

const PLOT_TOP_PAD: f32 = 10.0;
const PLOT_RIGHT_PAD: f32 = 14.0;
const HOVER_DOT_SIZE: Pixels = px(6.0);
const HOVER_HALO_SIZE: Pixels = px(14.0);
const BAR_HOVER_DIM: f32 = 0.35;

/// Which root hierarchy (`Assets` or `Liabilities`) is displayed in the Balance
/// Sheet line chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BalanceSheetMode {
    #[default]
    Assets,
    Liabilities,
}

impl BalanceSheetMode {
    pub const ALL: [Self; 2] = [Self::Assets, Self::Liabilities];

    pub fn label(self) -> &'static str {
        match self {
            Self::Assets => "Assets",
            Self::Liabilities => "Liabilities",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Assets => "Total Assets",
            Self::Liabilities => "Total Liabilities",
        }
    }

    pub fn short_id(self) -> &'static str {
        match self {
            Self::Assets => "assets",
            Self::Liabilities => "liabilities",
        }
    }
}

/// Which root hierarchy (`Income` or `Expenses`) is displayed in the Income
/// Statement bar chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IncomeChartMode {
    #[default]
    Income,
    Expenses,
}

impl IncomeChartMode {
    pub const ALL: [Self; 2] = [Self::Income, Self::Expenses];

    pub fn label(self) -> &'static str {
        match self {
            Self::Income => "Income",
            Self::Expenses => "Expenses",
        }
    }

    pub fn short_id(self) -> &'static str {
        match self {
            Self::Income => "income",
            Self::Expenses => "expenses",
        }
    }
}

/// Tracks which currencies are currently selected in a chart's checklist.
///
/// All currencies default to selected; explicitly unchecked currencies stay
/// deselected across reloads and interval changes while newly added currencies
/// default to selected.
#[derive(Clone, Debug, Default)]
pub(crate) struct CurrencySelection {
    deselected: HashSet<SharedString>,
}

impl CurrencySelection {
    pub(crate) fn is_selected(&self, currency: &str) -> bool {
        !self.deselected.contains(currency)
    }

    pub(crate) fn set_selected(&mut self, currency: SharedString, selected: bool) -> bool {
        if selected {
            self.deselected.remove(&currency)
        } else {
            self.deselected.insert(currency)
        }
    }

    #[cfg(test)]
    pub(crate) fn selected_currencies<'a>(&self, currencies: &'a [String]) -> Vec<&'a str> {
        currencies
            .iter()
            .map(String::as_str)
            .filter(|c| self.is_selected(c))
            .collect()
    }
}

fn decimal_to_f64(value: Decimal) -> f64 {
    let mantissa = value.mantissa() as f64;
    let scale = 10_f64.powi(value.scale() as i32);
    mantissa / scale
}

fn format_amount(value: Decimal, currency: &str) -> SharedString {
    let rounded = value.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
    let raw = if rounded.is_zero() {
        "0".to_string()
    } else {
        rounded.to_string()
    };
    let mut builder = GroupedBuilder::new();
    builder.push_number_str(&raw);
    builder.push_str(" ");
    builder.push_str(currency);
    builder.finish().plain()
}

fn format_axis_tick(value: f64) -> SharedString {
    if value.abs() < 1e-9 {
        return "0".into();
    }
    let abs = value.abs();
    if abs >= 1_000_000_000.0 {
        return format!("{:.1}B", value / 1_000_000_000.0)
            .trim_end_matches(".0B")
            .replace(".0B", "B")
            .into();
    }
    if abs >= 1_000_000.0 {
        let s = format!("{:.1}", value / 1_000_000.0);
        let trimmed = s.strip_suffix(".0").unwrap_or(&s);
        return format!("{trimmed}M").into();
    }
    let rounded = value.round();
    if (value - rounded).abs() < 1e-6 {
        let mut builder = GroupedBuilder::new();
        builder.push_number_str(&format!("{}", rounded as i64));
        return builder.finish().plain();
    }
    let mut builder = GroupedBuilder::new();
    builder.push_number_str(&format!("{value:.2}"));
    builder.finish().plain()
}

/// Computes `target_count`-ish human-friendly ticks spanning `[min_val.min(0.0), max_val.max(0.0)]`
/// such that `0.0` is always an exact tick.
fn nice_y_ticks(
    min_val: f64,
    max_val: f64,
    target_count: usize,
) -> (f64, f64, Vec<(f64, SharedString)>) {
    let lo = min_val.min(0.0);
    let hi = max_val.max(0.0);
    let span = hi - lo;
    if !span.is_finite() || span <= 1e-9 {
        return (
            0.0,
            1.0,
            vec![(0.0, "0".into()), (0.5, "0.50".into()), (1.0, "1".into())],
        );
    }

    let raw_step = span / (target_count.max(2).saturating_sub(1)) as f64;
    let mag = 10_f64.powf(raw_step.log10().floor());
    let norm = raw_step / mag;
    let nice_mult = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 2.5 {
        2.5
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    let step = nice_mult * mag;

    let nice_lo = (lo / step).floor() * step;
    let nice_hi = (hi / step).ceil() * step;
    let count = (((nice_hi - nice_lo) / step).round() as usize).clamp(1, 12);
    let mut ticks = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let mut v = nice_lo + i as f64 * step;
        if v.abs() < step * 1e-6 {
            v = 0.0;
        }
        ticks.push((v, format_axis_tick(v)));
    }
    (nice_lo, nice_hi, ticks)
}

fn account_series_color(currency_ix: usize, theme: &Theme) -> Hsla {
    let palette = [
        theme.chart_2,
        theme.chart_1,
        theme.chart_3,
        theme.chart_4,
        theme.chart_5,
        theme.cyan,
        theme.magenta,
        theme.yellow,
    ];
    palette[currency_ix % palette.len()]
}

fn balance_sheet_series_color(mode: BalanceSheetMode, currency_ix: usize, theme: &Theme) -> Hsla {
    let assets_palette = [
        theme.blue,
        theme.cyan,
        theme.green,
        theme.chart_3,
        theme.chart_4,
        theme.magenta,
        theme.yellow,
    ];
    let liabilities_palette = [
        theme.red,
        theme.yellow,
        theme.magenta,
        theme.chart_4,
        theme.chart_5,
        theme.cyan,
        theme.blue,
    ];
    match mode {
        BalanceSheetMode::Assets => assets_palette[currency_ix % assets_palette.len()],
        BalanceSheetMode::Liabilities => {
            liabilities_palette[currency_ix % liabilities_palette.len()]
        }
    }
}

fn income_series_color(mode: IncomeChartMode, currency_ix: usize, theme: &Theme) -> Hsla {
    let income_palette = [
        theme.chart_bullish,
        theme.blue,
        theme.cyan,
        theme.chart_2,
        theme.chart_3,
        theme.yellow,
        theme.magenta,
    ];
    let expenses_palette = [
        theme.chart_bearish,
        theme.yellow,
        theme.magenta,
        theme.chart_4,
        theme.chart_5,
        theme.cyan,
        theme.blue,
    ];
    match mode {
        IncomeChartMode::Income => income_palette[currency_ix % income_palette.len()],
        IncomeChartMode::Expenses => expenses_palette[currency_ix % expenses_palette.len()],
    }
}

/// Currencies with at least one non-zero value in `mode` (falling back to all
/// `series.currencies` when every currency is zero across all dates).
pub(crate) fn balance_sheet_mode_currencies(
    series: &BalanceSheetSeries,
    mode: BalanceSheetMode,
) -> Vec<(usize, &str)> {
    let non_zero: Vec<(usize, &str)> = series
        .currencies
        .iter()
        .enumerate()
        .filter(|&(ix, _)| {
            series.points.iter().any(|pt| match mode {
                BalanceSheetMode::Assets => !pt.assets[ix].is_zero(),
                BalanceSheetMode::Liabilities => !pt.liabilities[ix].is_zero(),
            })
        })
        .map(|(ix, c)| (ix, c.as_str()))
        .collect();
    if non_zero.is_empty() {
        series
            .currencies
            .iter()
            .enumerate()
            .map(|(ix, c)| (ix, c.as_str()))
            .collect()
    } else {
        non_zero
    }
}

/// Currencies with at least one non-zero bucket in `mode` (falling back to all
/// `series.currencies` when every currency is zero across all buckets).
pub(crate) fn income_mode_currencies(
    series: &IncomeExpenseSeries,
    mode: IncomeChartMode,
) -> Vec<(usize, &str)> {
    let non_zero: Vec<(usize, &str)> = series
        .currencies
        .iter()
        .enumerate()
        .filter(|&(ix, _)| {
            series.buckets.iter().any(|b| match mode {
                IncomeChartMode::Income => !b.income[ix].is_zero(),
                IncomeChartMode::Expenses => !b.expenses[ix].is_zero(),
            })
        })
        .map(|(ix, c)| (ix, c.as_str()))
        .collect();
    if non_zero.is_empty() {
        series
            .currencies
            .iter()
            .enumerate()
            .map(|(ix, c)| (ix, c.as_str()))
            .collect()
    } else {
        non_zero
    }
}

/// Renders a legend item (small rounded swatch + series label).
fn render_legend_item(color: Hsla, label: impl Into<SharedString>, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    div()
        .h_flex()
        .items_center()
        .gap_1p5()
        .child(div().w(px(8.)).h(px(8.)).rounded(px(2.)).bg(color))
        .child(div().text_xs().text_color(muted).child(label.into()))
        .into_any_element()
}

/// Renders the currency checklist row for a chart header.
fn render_currency_checklist<F>(
    currencies: &[String],
    selection: &CurrencySelection,
    on_toggle: F,
    _cx: &App,
) -> AnyElement
where
    F: Fn(SharedString, bool, &mut Window, &mut App) + 'static,
{
    let on_toggle = Rc::new(on_toggle);
    div()
        .id("chart-currency-checklist")
        .test_support()
        .h_flex()
        .flex_wrap()
        .items_center()
        .gap_3()
        .children(currencies.iter().map(|currency| {
            let ccy: SharedString = currency.clone().into();
            let checked = selection.is_selected(currency);
            let on_toggle = on_toggle.clone();
            let click_ccy = ccy.clone();
            Checkbox::new((ElementId::from("chart-currency"), ccy.clone()))
                .small()
                .label(ccy)
                .checked(checked)
                .on_change(move |next_checked, window, cx| {
                    on_toggle(click_ccy.clone(), *next_checked, window, cx);
                })
        }))
        .into_any_element()
}

fn render_empty_chart_state(message: &'static str, height: Pixels, cx: &App) -> AnyElement {
    div()
        .h(height)
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(message)
        .into_any_element()
}

/// Renders the Balance Sheet line chart card (`Assets` or `Liabilities` vs time,
/// switched via the bottom `Assets | Liabilities` button group).
pub(crate) fn render_balance_sheet_chart<FM, FC>(
    series: &BalanceSheetSeries,
    mode: BalanceSheetMode,
    selection: &CurrencySelection,
    on_select_mode: FM,
    on_toggle_currency: FC,
    cx: &App,
) -> Option<AnyElement>
where
    FM: Fn(BalanceSheetMode, &mut Window, &mut App) + 'static,
    FC: Fn(SharedString, bool, &mut Window, &mut App) + 'static,
{
    if series.currencies.is_empty() || series.points.is_empty() {
        return None;
    }

    let theme = cx.theme();
    let active_currencies = balance_sheet_mode_currencies(series, mode);
    let checklist_currencies: Vec<String> = active_currencies
        .iter()
        .map(|(_, c)| (*c).to_string())
        .collect();
    let mut lines = Vec::new();
    let mut legend = Vec::new();

    for (palette_ix, &(ccy_ix, currency)) in active_currencies.iter().enumerate() {
        if !selection.is_selected(currency) {
            continue;
        }
        let color = balance_sheet_series_color(mode, palette_ix, theme);
        let label: SharedString = currency.to_string().into();
        let mut vals = Vec::with_capacity(series.points.len());
        let mut fmt = Vec::with_capacity(series.points.len());

        for pt in &series.points {
            let v = match mode {
                BalanceSheetMode::Assets => pt.assets[ccy_ix],
                BalanceSheetMode::Liabilities => pt.liabilities[ccy_ix],
            };
            vals.push(decimal_to_f64(v));
            fmt.push(format_amount(v, currency));
        }

        legend.push(render_legend_item(color, label.clone(), cx));
        lines.push(LineSeriesData {
            label,
            color,
            values: vals,
            formatted: fmt,
        });
    }

    let on_select_mode = Rc::new(on_select_mode);
    let mode_selector = ButtonGroup::new("balance-sheet-mode-group")
        .outline()
        .compact()
        .children(BalanceSheetMode::ALL.map(|item| {
            Button::new((ElementId::from("balance-sheet-mode"), item.short_id()))
                .xsmall()
                .label(item.label())
                .selected(item == mode)
        }))
        .on_click(move |selected: &Vec<usize>, window, cx| {
            if let Some(next) = selected
                .first()
                .and_then(|ix| BalanceSheetMode::ALL.get(*ix))
            {
                on_select_mode(*next, window, cx);
            }
        });

    let plot_height = px(180.);
    let body = if lines.is_empty() {
        render_empty_chart_state("Select at least one currency to display", plot_height, cx)
    } else {
        let dates: Vec<SharedString> = series
            .points
            .iter()
            .map(|p| p.date.to_string().into())
            .collect();
        let day_indices: Vec<i32> = series.points.iter().map(|p| p.day_index()).collect();
        div()
            .id("balance-sheet-plot-area")
            .test_support()
            .h(plot_height)
            .w_full()
            .child(TimeLinePlot::new(
                (ElementId::from("chart-plot"), mode.short_id()),
                dates,
                day_indices,
                lines,
            ))
            .into_any_element()
    };

    Some(
        table_card(cx)
            .id((ElementId::from("chart-card"), "balance-sheet"))
            .test_support()
            .w_full()
            .min_w(px(480.))
            .v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .child(div().text_sm().font_medium().child(mode.title()))
                            .children(legend),
                    )
                    .child(render_currency_checklist(
                        &checklist_currencies,
                        selection,
                        on_toggle_currency,
                        cx,
                    )),
            )
            .child(body)
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .pt_0p5()
                    .child(mode_selector),
            )
            .into_any_element(),
    )
}

/// Renders the Account View balance line chart card (account balance vs time).
pub(crate) fn render_account_balance_chart<F>(
    account: &SharedString,
    series: &AccountBalanceSeries,
    selection: &CurrencySelection,
    on_toggle_currency: F,
    cx: &App,
) -> Option<AnyElement>
where
    F: Fn(SharedString, bool, &mut Window, &mut App) + 'static,
{
    if series.currencies.is_empty() || series.points.is_empty() {
        return None;
    }

    let theme = cx.theme();
    let mut lines = Vec::new();
    let mut legend = Vec::new();

    for (ccy_ix, currency) in series.currencies.iter().enumerate() {
        if !selection.is_selected(currency) {
            continue;
        }
        let color = account_series_color(ccy_ix, theme);
        let label: SharedString = currency.clone().into();
        let mut vals = Vec::with_capacity(series.points.len());
        let mut fmt = Vec::with_capacity(series.points.len());
        for pt in &series.points {
            let b = pt.balances[ccy_ix];
            vals.push(decimal_to_f64(b));
            fmt.push(format_amount(b, currency));
        }
        legend.push(render_legend_item(color, label.clone(), cx));
        lines.push(LineSeriesData {
            label,
            color,
            values: vals,
            formatted: fmt,
        });
    }

    let plot_height = px(156.);
    let body = if lines.is_empty() {
        render_empty_chart_state("Select at least one currency to display", plot_height, cx)
    } else {
        let dates: Vec<SharedString> = series
            .points
            .iter()
            .map(|p| p.date.to_string().into())
            .collect();
        let day_indices: Vec<i32> = series.points.iter().map(|p| p.day_index()).collect();
        div()
            .id("account-balance-plot-area")
            .test_support()
            .h(plot_height)
            .w_full()
            .child(TimeLinePlot::new(
                (ElementId::from("account-balance-plot"), account.clone()),
                dates,
                day_indices,
                lines,
            ))
            .into_any_element()
    };

    Some(
        table_card(cx)
            .id((ElementId::from("chart-card"), account.clone()))
            .test_support()
            .w_full()
            .v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .child(div().text_sm().font_medium().child("Account Balance"))
                            .children(legend),
                    )
                    .child(render_currency_checklist(
                        &series.currencies,
                        selection,
                        on_toggle_currency,
                        cx,
                    )),
            )
            .child(body)
            .into_any_element(),
    )
}

/// Renders the Income Statement bar chart card (`Income` or `Expenses` by
/// `Daily` / `Monthly` / `Quarterly` / `Yearly`, switched via the bottom
/// `Income | Expenses` button group).
pub(crate) fn render_income_expense_chart<FM, FI, FC>(
    series: &IncomeExpenseSeries,
    (mode, interval): (IncomeChartMode, TimeInterval),
    selection: &CurrencySelection,
    on_select_mode: FM,
    on_select_interval: FI,
    on_toggle_currency: FC,
    cx: &App,
) -> Option<AnyElement>
where
    FM: Fn(IncomeChartMode, &mut Window, &mut App) + 'static,
    FI: Fn(TimeInterval, &mut Window, &mut App) + 'static,
    FC: Fn(SharedString, bool, &mut Window, &mut App) + 'static,
{
    if series.currencies.is_empty() || series.buckets.is_empty() {
        return None;
    }

    let theme = cx.theme();
    let active_currencies = income_mode_currencies(series, mode);
    let checklist_currencies: Vec<String> = active_currencies
        .iter()
        .map(|(_, c)| (*c).to_string())
        .collect();
    let mut bars = Vec::new();
    let mut legend = Vec::new();

    for (palette_ix, &(ccy_ix, currency)) in active_currencies.iter().enumerate() {
        if !selection.is_selected(currency) {
            continue;
        }
        let color = income_series_color(mode, palette_ix, theme);
        let label: SharedString = currency.to_string().into();
        let mut vals = Vec::with_capacity(series.buckets.len());
        let mut fmt = Vec::with_capacity(series.buckets.len());

        for bucket in &series.buckets {
            let v = match mode {
                IncomeChartMode::Income => bucket.income[ccy_ix],
                IncomeChartMode::Expenses => bucket.expenses[ccy_ix],
            };
            vals.push(decimal_to_f64(v));
            fmt.push(format_amount(v, currency));
        }

        legend.push(render_legend_item(color, label.clone(), cx));
        bars.push(BarSeriesData {
            label,
            color,
            values: vals,
            formatted: fmt,
        });
    }

    let on_select_interval = Rc::new(on_select_interval);
    let interval_selector = ButtonGroup::new("income-interval-group")
        .outline()
        .compact()
        .children(TimeInterval::ALL.map(|item| {
            Button::new((ElementId::from("income-interval"), item.short_id()))
                .xsmall()
                .label(item.label())
                .selected(item == interval)
        }))
        .on_click(move |selected: &Vec<usize>, window, cx| {
            if let Some(next) = selected.first().and_then(|ix| TimeInterval::ALL.get(*ix)) {
                on_select_interval(*next, window, cx);
            }
        });

    let on_select_mode = Rc::new(on_select_mode);
    let mode_selector = ButtonGroup::new("income-chart-mode-group")
        .outline()
        .compact()
        .children(IncomeChartMode::ALL.map(|item| {
            Button::new((ElementId::from("income-chart-mode"), item.short_id()))
                .xsmall()
                .label(item.label())
                .selected(item == mode)
        }))
        .on_click(move |selected: &Vec<usize>, window, cx| {
            if let Some(next) = selected
                .first()
                .and_then(|ix| IncomeChartMode::ALL.get(*ix))
            {
                on_select_mode(*next, window, cx);
            }
        });

    let plot_height = px(180.);
    let body = if bars.is_empty() {
        render_empty_chart_state("Select at least one currency to display", plot_height, cx)
    } else {
        let labels: Vec<SharedString> = series
            .buckets
            .iter()
            .map(|b| b.label.clone().into())
            .collect();
        div()
            .id("income-expense-plot-area")
            .test_support()
            .h(plot_height)
            .w_full()
            .child(IncomeExpenseBarPlot::new(
                (
                    ElementId::from("income-expense-plot"),
                    format!("{}-{}", mode.short_id(), interval.short_id()),
                ),
                labels,
                bars,
            ))
            .into_any_element()
    };

    Some(
        table_card(cx)
            .id((ElementId::from("chart-card"), "income"))
            .test_support()
            .w_full()
            .min_w(px(480.))
            .v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .child(div().text_sm().font_medium().child(mode.label()))
                            .child(interval_selector)
                            .children(legend),
                    )
                    .child(render_currency_checklist(
                        &checklist_currencies,
                        selection,
                        on_toggle_currency,
                        cx,
                    )),
            )
            .child(body)
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .pt_0p5()
                    .child(mode_selector),
            )
            .into_any_element(),
    )
}

struct LineSeriesData {
    label: SharedString,
    color: Hsla,
    values: Vec<f64>,
    formatted: Vec<SharedString>,
}

struct TimeLinePlot {
    id: ElementId,
    dates: Vec<SharedString>,
    day_indices: Vec<i32>,
    series: Vec<LineSeriesData>,
    y_min: f64,
    y_max: f64,
    y_ticks: Vec<(f64, SharedString)>,
    y_gutter: f32,
}

impl TimeLinePlot {
    fn new(
        id: impl Into<ElementId>,
        dates: Vec<SharedString>,
        day_indices: Vec<i32>,
        series: Vec<LineSeriesData>,
    ) -> Self {
        let mut min_val = 0.0_f64;
        let mut max_val = 0.0_f64;
        for s in &series {
            for &v in &s.values {
                if v < min_val {
                    min_val = v;
                }
                if v > max_val {
                    max_val = v;
                }
            }
        }
        let (y_min, y_max, y_ticks) = nice_y_ticks(min_val, max_val, 5);
        Self {
            id: id.into(),
            dates,
            day_indices,
            series,
            y_min,
            y_max,
            y_ticks,
            y_gutter: 48.0,
        }
    }

    fn plot_rect(&self, bounds: Bounds<Pixels>) -> (f32, f32, f32, f32) {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let left = self.y_gutter;
        let right = (width - PLOT_RIGHT_PAD).max(left + 1.0);
        let top = PLOT_TOP_PAD;
        let bottom = (height - axis_gutter(px(TEXT_SIZE))).max(top + 1.0);
        (left, right, top, bottom)
    }

    fn x_for_day(&self, day: i32, left: f32, right: f32) -> f32 {
        let (Some(&first), Some(&last)) = (self.day_indices.first(), self.day_indices.last())
        else {
            return left;
        };
        if last <= first {
            (left + right) * 0.5
        } else {
            let t = (day - first) as f32 / (last - first) as f32;
            left + t * (right - left)
        }
    }

    fn y_scale(&self, top: f32, bottom: f32) -> ScaleLinear<f64> {
        ScaleLinear::new([self.y_min, self.y_max], [bottom, top])
    }
}

impl IntoElement for TimeLinePlot {
    type Element = PlotElement<Self>;

    fn into_element(self) -> Self::Element {
        PlotElement::new(self)
    }
}

impl Plot for TimeLinePlot {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn prepaint(
        &mut self,
        _bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut App,
    ) -> Vec<AnyElement> {
        let max_label = self
            .y_ticks
            .iter()
            .map(|(_, label)| measure_text_width(label, px(TEXT_SIZE), window))
            .fold(0.0_f32, f32::max);
        self.y_gutter = (max_label + 12.0).ceil().max(32.0);
        Vec::new()
    }

    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        if self.dates.is_empty() || self.series.is_empty() {
            return;
        }
        let (left, right, top, bottom) = self.plot_rect(bounds);
        let y_scale = self.y_scale(top, bottom);
        let (border_color, grid_color, label_color, dot_stroke) = {
            let theme = cx.theme();
            (
                theme.border,
                theme.chart_grid,
                theme.muted_foreground,
                theme.tokens.table,
            )
        };

        let plot_bounds = Bounds {
            origin: bounds.origin + point(px(left), px(0.)),
            size: Size::new(px(right - left), bounds.size.height),
        };

        // Horizontal dashed grid lines + crisp zero baseline.
        let mut dashed_ys = Vec::with_capacity(self.y_ticks.len());
        let mut zero_y = None;
        for &(val, _) in &self.y_ticks {
            if let Some(y) = y_scale.tick(&val) {
                if val.abs() < 1e-9 {
                    zero_y = Some(px(y));
                } else {
                    dashed_ys.push(px(y));
                }
            }
        }
        Grid::new()
            .y(dashed_ys)
            .stroke(grid_color)
            .dash_array(&[px(3.), px(3.)])
            .paint(&plot_bounds, window);
        if let Some(zy) = zero_y {
            Grid::new()
                .y([zy])
                .stroke(border_color)
                .paint(&plot_bounds, window);
        }

        // Y-axis tick labels in the left gutter.
        let y_labels: Vec<Text> = self
            .y_ticks
            .iter()
            .filter_map(|(val, text)| {
                let y = y_scale.tick(val)?;
                Some(
                    Text::new(
                        text.clone(),
                        point(px(left - 6.0), px(y - TEXT_SIZE * 0.5)),
                        label_color,
                    )
                    .align(TextAlign::Right),
                )
            })
            .collect();
        PlotLabel::new(y_labels).paint(&bounds, window, cx);

        // X-axis line and evenly spaced calendar date ticks.
        let mut x_labels = Vec::new();
        if let (Some(&first_day), Some(&last_day)) =
            (self.day_indices.first(), self.day_indices.last())
        {
            if last_day <= first_day {
                if let Some(text) = format_day_index(first_day) {
                    x_labels.push(
                        AxisText::new(text, px((right - left) * 0.5), label_color)
                            .align(TextAlign::Center),
                    );
                }
            } else {
                let span_days = (last_day - first_day) as usize;
                let max_ticks = (((right - left) / 105.0).floor() as usize).clamp(2, 7);
                let tick_count = max_ticks.min(span_days + 1);
                for i in 0..tick_count {
                    let frac = i as f32 / (tick_count - 1) as f32;
                    let day = first_day + ((last_day - first_day) as f32 * frac).round() as i32;
                    if let Some(text) = format_day_index(day) {
                        let align = if i == 0 {
                            TextAlign::Left
                        } else if i + 1 == tick_count {
                            TextAlign::Right
                        } else {
                            TextAlign::Center
                        };
                        x_labels.push(
                            AxisText::new(text, px(frac * (right - left)), label_color)
                                .align(align),
                        );
                    }
                }
            }
        }
        PlotAxis::new()
            .stroke(border_color)
            .x(bottom)
            .x_label(x_labels)
            .paint(&plot_bounds, window, cx);

        // Paint each active series line.
        let show_dots = self.dates.len() <= 32;
        let xs: Vec<f32> = self
            .day_indices
            .iter()
            .map(|&d| self.x_for_day(d, left, right))
            .collect();
        let caches = PathCaches::for_paint(self.id.clone(), window, cx);
        caches.update(cx, |caches, _| {
            for (s_ix, series) in self.series.iter().enumerate() {
                let pts: Vec<(f32, Option<f32>)> = xs
                    .iter()
                    .copied()
                    .zip(series.values.iter().map(|v| y_scale.tick(v)))
                    .collect();
                let mut line = Line::new()
                    .data(pts)
                    .x(|(x, _)| Some(*x))
                    .y(|(_, y)| *y)
                    .stroke(series.color)
                    .curve(Curve::Linear)
                    .stroke_width(px(2.0));
                if show_dots {
                    line = line
                        .dot()
                        .dot_size(px(5.0))
                        .dot_fill(series.color)
                        .dot_stroke(dot_stroke);
                }
                line.paint_cached(&bounds, caches.slot(s_ix), window);
            }
        });
    }

    fn tooltip_state(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _cx: &App,
    ) -> Option<TooltipState> {
        if self.dates.is_empty() || self.series.is_empty() {
            return None;
        }
        let (left, right, top, bottom) = self.plot_rect(bounds);
        let cx_x = f32::from(position.x);
        let cx_y = f32::from(position.y);
        if cx_x < left || cx_x > right || cx_y < 0.0 || cx_y > bottom {
            return None;
        }

        let (best_ix, best_x) = self
            .day_indices
            .iter()
            .enumerate()
            .map(|(ix, &day)| (ix, self.x_for_day(day, left, right)))
            .min_by(|(_, xa), (_, xb)| (xa - cx_x).abs().total_cmp(&(xb - cx_x).abs()))?;

        let y_scale = self.y_scale(top, bottom);
        let dots: Vec<Point<Pixels>> = self
            .series
            .iter()
            .filter_map(|s| {
                let y = y_scale.tick(s.values.get(best_ix)?)?;
                Some(point(px(best_x), px(y)))
            })
            .collect();

        Some(TooltipState::new(
            best_ix,
            point(px(best_x), position.y),
            dots,
        ))
    }

    fn tooltip(
        &self,
        state: &TooltipState,
        cursor: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let date = self.dates.get(state.index)?.clone();
        let (left, right, _, bottom) = self.plot_rect(bounds);
        let bg = cx.theme().background;

        let dots = state.dots.iter().zip(&self.series).map(|(&pt, s)| {
            Dot::new(pt)
                .size(HOVER_DOT_SIZE)
                .halo(HOVER_HALO_SIZE)
                .stroke(bg)
                .fill(s.color)
        });

        let mut tooltip = Tooltip::new(cursor, bounds.size)
            .gap(px(10.))
            .cross_line(
                CrossLine::new(state.cross_line)
                    .h_span(left, right - left)
                    .span(0.0, bottom),
            )
            .dots(dots)
            .title(date);

        for s in &self.series {
            if let Some(val) = s.formatted.get(state.index) {
                tooltip = tooltip.row(s.color, s.label.clone(), val.clone());
            }
        }

        Some(tooltip.into_any_element())
    }
}

struct BarSeriesData {
    label: SharedString,
    color: Hsla,
    values: Vec<f64>,
    formatted: Vec<SharedString>,
}

struct IncomeExpenseBarPlot {
    id: ElementId,
    labels: Vec<SharedString>,
    series: Vec<BarSeriesData>,
    y_min: f64,
    y_max: f64,
    y_ticks: Vec<(f64, SharedString)>,
    y_gutter: f32,
    hover: Option<(usize, f32)>,
}

impl IncomeExpenseBarPlot {
    fn new(
        id: impl Into<ElementId>,
        labels: Vec<SharedString>,
        series: Vec<BarSeriesData>,
    ) -> Self {
        let mut min_val = 0.0_f64;
        let mut max_val = 0.0_f64;
        for s in &series {
            for &v in &s.values {
                if v < min_val {
                    min_val = v;
                }
                if v > max_val {
                    max_val = v;
                }
            }
        }
        let (y_min, y_max, y_ticks) = nice_y_ticks(min_val, max_val, 5);
        Self {
            id: id.into(),
            labels,
            series,
            y_min,
            y_max,
            y_ticks,
            y_gutter: 48.0,
            hover: None,
        }
    }

    fn plot_rect(&self, bounds: Bounds<Pixels>) -> (f32, f32, f32, f32) {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let left = self.y_gutter;
        let right = (width - PLOT_RIGHT_PAD).max(left + 1.0);
        let top = PLOT_TOP_PAD;
        let bottom = (height - axis_gutter(px(TEXT_SIZE))).max(top + 1.0);
        (left, right, top, bottom)
    }

    fn y_scale(&self, top: f32, bottom: f32) -> ScaleLinear<f64> {
        ScaleLinear::new([self.y_min, self.y_max], [bottom, top])
    }
}

impl IntoElement for IncomeExpenseBarPlot {
    type Element = PlotElement<Self>;

    fn into_element(self) -> Self::Element {
        PlotElement::new(self)
    }
}

impl Plot for IncomeExpenseBarPlot {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn prepaint(
        &mut self,
        _bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut App,
    ) -> Vec<AnyElement> {
        let max_label = self
            .y_ticks
            .iter()
            .map(|(_, label)| measure_text_width(label, px(TEXT_SIZE), window))
            .fold(0.0_f32, f32::max);
        self.y_gutter = (max_label + 12.0).ceil().max(32.0);
        Vec::new()
    }

    fn hover(&mut self, hover: Option<&PlotHover>, _window: &mut Window, _cx: &mut App) {
        self.hover = hover.map(|h| (h.state().index, h.progress()));
    }

    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let n = self.labels.len();
        let m = self.series.len();
        if n == 0 || m == 0 {
            return;
        }
        let (left, right, top, bottom) = self.plot_rect(bounds);
        let y_scale = self.y_scale(top, bottom);
        let y_zero = y_scale.tick(&0.0).unwrap_or(bottom);
        let (border_color, grid_color, label_color) = {
            let theme = cx.theme();
            (theme.border, theme.chart_grid, theme.muted_foreground)
        };

        let plot_bounds = Bounds {
            origin: bounds.origin + point(px(left), px(0.)),
            size: Size::new(px(right - left), bounds.size.height),
        };

        // Horizontal dashed grid lines + solid y = 0 baseline.
        let mut dashed_ys = Vec::with_capacity(self.y_ticks.len());
        let mut zero_tick_y = None;
        for &(val, _) in &self.y_ticks {
            if let Some(y) = y_scale.tick(&val) {
                if val.abs() < 1e-9 {
                    zero_tick_y = Some(px(y));
                } else {
                    dashed_ys.push(px(y));
                }
            }
        }
        Grid::new()
            .y(dashed_ys)
            .stroke(grid_color)
            .dash_array(&[px(3.), px(3.)])
            .paint(&plot_bounds, window);
        if let Some(zy) = zero_tick_y {
            Grid::new()
                .y([zy])
                .stroke(border_color)
                .paint(&plot_bounds, window);
        }

        // Y-axis tick labels.
        let y_labels: Vec<Text> = self
            .y_ticks
            .iter()
            .filter_map(|(val, text)| {
                let y = y_scale.tick(val)?;
                Some(
                    Text::new(
                        text.clone(),
                        point(px(left - 6.0), px(y - TEXT_SIZE * 0.5)),
                        label_color,
                    )
                    .align(TextAlign::Right),
                )
            })
            .collect();
        PlotLabel::new(y_labels).paint(&bounds, window, cx);

        // X-axis line and auto-thinned bucket labels.
        let plot_w = right - left;
        let slot_w = plot_w / n as f32;
        let max_x_ticks = ((plot_w / 88.0).floor() as usize).clamp(2, 8);
        let mut x_labels = Vec::new();
        if n <= max_x_ticks {
            for (ix, label) in self.labels.iter().enumerate() {
                let tick_x = (ix as f32 + 0.5) * slot_w;
                x_labels.push(
                    AxisText::new(label.clone(), px(tick_x), label_color).align(TextAlign::Center),
                );
            }
        } else {
            for t in 0..max_x_ticks {
                let ix = ((t * (n - 1)) as f32 / (max_x_ticks - 1) as f32).round() as usize;
                let tick_x = (ix as f32 + 0.5) * slot_w;
                let align = if t == 0 && tick_x < 28.0 {
                    TextAlign::Left
                } else if t + 1 == max_x_ticks && (plot_w - tick_x) < 28.0 {
                    TextAlign::Right
                } else {
                    TextAlign::Center
                };
                let anchor = match align {
                    TextAlign::Left => tick_x.max(0.0),
                    TextAlign::Right => tick_x.min(plot_w),
                    _ => tick_x,
                };
                x_labels.push(
                    AxisText::new(self.labels[ix].clone(), px(anchor), label_color).align(align),
                );
            }
        }
        PlotAxis::new()
            .stroke(border_color)
            .x(bottom)
            .x_label(x_labels)
            .paint(&plot_bounds, window, cx);

        // Grouped vertical bars per bucket.
        let group_w = (slot_w * 0.72).min(64.0).max(slot_w.min(2.0));
        let bar_gap = if group_w / m as f32 >= 6.0 { 2.0 } else { 0.0 };
        let sub_w = ((group_w - bar_gap * m.saturating_sub(1) as f32) / m as f32).max(1.5);
        let actual_group_w = sub_w * m as f32 + bar_gap * m.saturating_sub(1) as f32;
        let hover = self.hover;
        let radius = if sub_w >= 4.0 { px(2.0) } else { px(0.0) };

        #[derive(Clone, Copy)]
        struct BarItem {
            x: f32,
            y: f32,
            color: Hsla,
        }

        let mut pos_items = Vec::new();
        let mut neg_items = Vec::new();

        for bucket_ix in 0..n {
            let center_x = left + (bucket_ix as f32 + 0.5) * slot_w;
            let start_x = center_x - actual_group_w * 0.5;
            let dim = match hover {
                Some((hov_ix, progress)) if hov_ix != bucket_ix => 1.0 - BAR_HOVER_DIM * progress,
                _ => 1.0,
            };
            for (s_ix, series) in self.series.iter().enumerate() {
                let val = series.values[bucket_ix];
                if val.abs() < 1e-9 {
                    continue;
                }
                let Some(y_val) = y_scale.tick(&val) else {
                    continue;
                };
                let item = BarItem {
                    x: start_x + s_ix as f32 * (sub_w + bar_gap),
                    y: y_val,
                    color: series.color.opacity(series.color.a * dim),
                };
                if val >= 0.0 {
                    pos_items.push(item);
                } else {
                    neg_items.push(item);
                }
            }
        }

        if !pos_items.is_empty() {
            Bar::new()
                .data(pos_items)
                .alignment(BarAlignment::Bottom)
                .band_width(sub_w)
                .base(move |_| y_zero)
                .cross(|item| Some(item.x))
                .value(|item| Some(item.y))
                .fill(|item, _, _| item.color)
                .corner_radii(Corners {
                    top_left: radius,
                    top_right: radius,
                    bottom_right: px(0.),
                    bottom_left: px(0.),
                })
                .paint(&bounds, window, cx);
        }
        if !neg_items.is_empty() {
            Bar::new()
                .data(neg_items)
                .alignment(BarAlignment::Bottom)
                .band_width(sub_w)
                .base(move |_| y_zero)
                .cross(|item| Some(item.x))
                .value(|item| Some(item.y))
                .fill(|item, _, _| item.color)
                .corner_radii(Corners {
                    top_left: px(0.),
                    top_right: px(0.),
                    bottom_right: radius,
                    bottom_left: radius,
                })
                .paint(&bounds, window, cx);
        }
    }

    fn tooltip_state(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _cx: &App,
    ) -> Option<TooltipState> {
        let n = self.labels.len();
        if n == 0 || self.series.is_empty() {
            return None;
        }
        let (left, right, _, bottom) = self.plot_rect(bounds);
        let cx_x = f32::from(position.x);
        let cx_y = f32::from(position.y);
        if cx_x < left || cx_x > right || cx_y < 0.0 || cx_y > bottom {
            return None;
        }

        let slot_w = (right - left) / n as f32;
        let ix = (((cx_x - left) / slot_w).floor() as isize).clamp(0, n as isize - 1) as usize;
        let center_x = left + (ix as f32 + 0.5) * slot_w;

        Some(TooltipState::new(
            ix,
            point(px(center_x), position.y),
            Vec::new(),
        ))
    }

    fn tooltip(
        &self,
        state: &TooltipState,
        cursor: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<AnyElement> {
        let n = self.labels.len();
        let label = self.labels.get(state.index)?.clone();
        let (left, right, _, bottom) = self.plot_rect(bounds);
        let slot_w = (right - left) / n.max(1) as f32;
        let band_w = slot_w.clamp(3.0, 76.0);

        let mut tooltip = Tooltip::new(cursor, bounds.size)
            .gap(px(10.))
            .cross_line(
                CrossLine::new(state.cross_line)
                    .band(px(band_w))
                    .span(0.0, bottom),
            )
            .title(label);

        for s in &self.series {
            if let Some(val) = s.formatted.get(state.index) {
                tooltip = tooltip.row(s.color, s.label.clone(), val.clone());
            }
        }

        Some(tooltip.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use lumi::web::{
        BalanceSheetPoint, BalanceSheetSeries, IncomeExpenseBucket, IncomeExpenseSeries, NaiveDate,
        TimeInterval,
    };
    use rust_decimal::Decimal;

    use super::{
        BalanceSheetMode, IncomeChartMode, balance_sheet_mode_currencies, format_amount,
        income_mode_currencies,
    };

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn test_format_amount_rounds_hover_numbers_to_at_most_two_decimal_places() {
        assert_eq!(
            format_amount(dec("4554.50"), "USD").as_ref(),
            "4 554.50 USD"
        );
        assert_eq!(
            format_amount(dec("1123456.7891"), "USD").as_ref(),
            "1 123 456.79 USD"
        );
        assert_eq!(
            format_amount(dec("2536.59406652"), "USD").as_ref(),
            "2 536.59 USD"
        );
        assert_eq!(
            format_amount(dec("-123.455"), "EUR").as_ref(),
            "-123.46 EUR"
        );
        assert_eq!(format_amount(dec("12.3000"), "USD").as_ref(), "12.30 USD");
        assert_eq!(format_amount(dec("10"), "VTI").as_ref(), "10 VTI");
        assert_eq!(format_amount(dec("10.5"), "USD").as_ref(), "10.5 USD");
        assert_eq!(format_amount(dec("-0.004"), "USD").as_ref(), "0 USD");
        assert_eq!(format_amount(Decimal::ZERO, "USD").as_ref(), "0 USD");
    }

    #[test]
    fn test_mode_currencies_select_nonzero_hierarchy_with_zero_fallback() {
        let bs = BalanceSheetSeries {
            currencies: vec!["EUR".into(), "USD".into()],
            points: vec![BalanceSheetPoint {
                date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                assets: vec![dec("-300.00"), dec("1000.00")],
                liabilities: vec![dec("0"), dec("-80.00")],
            }],
        };
        assert_eq!(
            balance_sheet_mode_currencies(&bs, BalanceSheetMode::Assets),
            vec![(0, "EUR"), (1, "USD")]
        );
        assert_eq!(
            balance_sheet_mode_currencies(&bs, BalanceSheetMode::Liabilities),
            vec![(1, "USD")]
        );

        let all_zero_liab = BalanceSheetSeries {
            currencies: vec!["USD".into()],
            points: vec![BalanceSheetPoint {
                date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                assets: vec![dec("500.00")],
                liabilities: vec![dec("0")],
            }],
        };
        assert_eq!(
            balance_sheet_mode_currencies(&all_zero_liab, BalanceSheetMode::Liabilities),
            vec![(0, "USD")]
        );

        let inc = IncomeExpenseSeries {
            interval: TimeInterval::Monthly,
            currencies: vec!["EUR".into(), "USD".into()],
            buckets: vec![IncomeExpenseBucket {
                label: "2025-01".into(),
                income: vec![dec("0"), dec("3000.00")],
                expenses: vec![dec("300.00"), dec("125.50")],
            }],
        };
        assert_eq!(
            income_mode_currencies(&inc, IncomeChartMode::Income),
            vec![(1, "USD")]
        );
        assert_eq!(
            income_mode_currencies(&inc, IncomeChartMode::Expenses),
            vec![(0, "EUR"), (1, "USD")]
        );
    }
}

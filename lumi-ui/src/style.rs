use std::sync::Arc;

#[cfg(test)]
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::scroll::{Scrollbar, ScrollbarAxis};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::component::{
    ActiveTheme as _, InteractiveElementExt as _, Sizable as _, Size, StyledExt as _, Theme,
    window_paddings,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, Decorations, Div, Element, ElementId, Entity, Font, FontWeight,
    GlobalElementId, Hsla, InspectorElementId, InteractiveElement as _, IntoElement, LayoutId,
    ParentElement as _, Pixels, ScrollHandle, SharedString, Stateful,
    StatefulInteractiveElement as _, Style, Styled as _, Window, WindowTextSystem, div, px,
    relative, rems,
};

use crate::number::{GroupedText, shape_text_width};

/// Customizes the global GPUI Kit theme tokens for lumi's report tables and
/// editors.
pub(crate) fn configure_theme(cx: &mut App) {
    Theme::update(cx, |theme| {
        // Alternating row backgrounds already separate rows, so omit the thin
        // horizontal divider between them.
        theme.table_row_border = theme.transparent;
        // Deeper grey header and alternating white/grey rows across all tables.
        theme.table_head = theme.secondary;
        theme.table_head_foreground = theme.secondary_foreground;
        theme.table_even = if theme.is_dark() {
            theme.muted.opacity(0.45)
        } else {
            theme.muted
        };
        theme.table_active = theme
            .blue
            .opacity(if theme.is_dark() { 0.22 } else { 0.14 });
        // Use a translucent active-line tint so non-error lines match the
        // default neutral highlight while light-red error backgrounds remain
        // visible when the cursor sits on an error line.
        let active_line = theme
            .foreground
            .opacity(if theme.is_dark() { 0.06 } else { 0.04 });
        Arc::make_mut(&mut theme.highlight_theme)
            .style
            .editor_active_line = Some(active_line);
    });
}

/// Background surface of the code editor, matching GPUI Kit's internal
/// `Theme::editor_background`.
pub(crate) fn editor_bg(cx: &App) -> Hsla {
    let theme = cx.theme();
    theme
        .highlight_theme
        .style
        .editor_background
        .unwrap_or(theme.background)
}

/// Soft red row highlight painted behind lines with `ErrorLevel::Error` in the
/// source editor.
pub(crate) fn editor_error_line_bg(cx: &App) -> Hsla {
    let theme = cx.theme();
    theme.red.opacity(if theme.is_dark() { 0.18 } else { 0.12 })
}

/// A sharp-cornered paper-card container for report tables.
pub(crate) fn table_card(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .border_1()
        .border_color(theme.border)
        .bg(theme.tokens.table)
        .shadow_sm()
}

/// The shared header row style for `DataTable` delegates, matching
/// `TrieReport`'s medium-weight header.
pub(crate) fn table_header() -> Stateful<Div> {
    div().id("header").font_medium()
}

/// Base row container for `DataTable` delegates without a horizontal bottom
/// border reservation so vertical column separators span the full row height.
pub(crate) fn table_row(row_ix: usize) -> Stateful<Div> {
    div().id(("row", row_ix)).border_b_0()
}

/// Alternating white/grey row background by logical row index.
pub(crate) fn table_row_bg(row_ix: usize, cx: &App) -> Hsla {
    if row_ix.is_multiple_of(2) {
        cx.theme().table
    } else {
        cx.theme().table_even
    }
}

/// Vertical column separator color matching the base (white) row surface so it
/// shows on the grey header and grey alternating rows while blending into white rows.
pub(crate) fn table_col_separator_color(cx: &App) -> Hsla {
    cx.theme().table
}

fn col_separator_line(
    top: Pixels,
    bottom: Pixels,
    left: Pixels,
    #[cfg_attr(not(test), allow(unused_variables))] id: impl FnOnce() -> ElementId,
    cx: &App,
) -> AnyElement {
    let line = div()
        .absolute()
        .top(top)
        .bottom(bottom)
        .left(left)
        .w(px(1.))
        .bg(table_col_separator_color(cx));
    #[cfg(test)]
    let line = line.id(id()).test_support();
    line.into_any_element()
}

/// Header cell for a `Size::Small` `DataTable`, drawing `1px` vertical white
/// column separators between adjacent columns across the full header row height.
pub(crate) fn data_table_th(columns: &[Column], col_ix: usize, cx: &App) -> Div {
    let pad = Size::Small.table_cell_padding();
    div()
        .size_full()
        .child(columns[col_ix].name.clone())
        .when(col_ix > 0, |this| {
            this.child(col_separator_line(
                -pad.top,
                -pad.bottom,
                -pad.left,
                || ("col-sep-head", col_ix).into(),
                cx,
            ))
        })
}

/// Body cell wrapper for a `Size::Small` `DataTable`, drawing `1px` vertical
/// white column separators between adjacent columns aligned with [`data_table_th`].
pub(crate) fn data_table_td(
    #[cfg_attr(not(test), allow(unused_variables))] row_ix: usize,
    col_ix: usize,
    cell: impl IntoElement,
    cx: &App,
) -> Div {
    let pad = Size::Small.table_cell_padding();
    div().size_full().child(cell).when(col_ix > 0, |this| {
        this.child(col_separator_line(
            -pad.top,
            -pad.bottom,
            -pad.left,
            || {
                #[cfg(test)]
                {
                    (
                        ElementId::from("col-sep-cell"),
                        SharedString::from(format!("{row_ix}:{col_ix}")),
                    )
                        .into()
                }
                #[cfg(not(test))]
                {
                    "col-sep-cell".into()
                }
            },
            cx,
        ))
    })
}

/// Horizontal inset occupied by client-side window decorations (shadow padding +
/// `1px` border on untiled edges on Linux; `0px` on macOS, Windows, Web, and
/// `TestWindow`).
pub(crate) fn window_chrome_horizontal_inset(window: &Window) -> Pixels {
    let paddings = window_paddings(window);
    let border = match window.window_decorations() {
        Decorations::Server => px(0.),
        Decorations::Client { tiling } => {
            (if tiling.left { px(0.) } else { px(1.) })
                + (if tiling.right { px(0.) } else { px(1.) })
        }
    };
    paddings.left + paddings.right + border
}

/// Fixed horizontal width outside a report's content box inside [`LumiApp`]:
/// window decorations + `Sidebar` (`w_48` = `12rem`) + report horizontal
/// padding (`px_6` / `p_6` = `3rem`).
pub(crate) fn report_horizontal_overhead(window: &Window) -> Pixels {
    let rem = window.rem_size();
    window_chrome_horizontal_inset(window) + rems(12.).to_pixels(rem) + rems(3.).to_pixels(rem)
}

/// Inner horizontal width available to a report's content box inside [`LumiApp`].
pub(crate) fn available_report_content_width(window: &Window) -> Pixels {
    (window.viewport_size().width - report_horizontal_overhead(window)).max(px(0.))
}

/// Horizontal-only scroll area that shows a bottom [`Scrollbar`] when `content`
/// is wider than the container, without intercepting vertical mouse-wheel
/// scrolling.
pub(crate) fn horizontal_scroll_area(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let id = id.into();
    let scroll_handle = window
        .use_keyed_state((id.clone(), "hscroll-handle"), cx, |_, _| {
            ScrollHandle::new()
        })
        .read(cx)
        .clone();
    let area = div()
        .id(id.clone())
        .w_full()
        .flex()
        .flex_row()
        .overflow_x_scroll()
        .lock_scroll_axis()
        .track_scroll(&scroll_handle)
        .child(content);
    #[cfg(test)]
    let area = area.test_support();
    div()
        .relative()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .child(area)
        .child(
            div().absolute().inset_0().child(
                Scrollbar::new(&scroll_handle)
                    .id((id, "scrollbar"))
                    .axis(ScrollbarAxis::Horizontal)
                    .viewport_from_layout(),
            ),
        )
}

/// Shared font and spacing metrics for measuring natural content widths of
/// `Size::Small` report table columns.
pub(crate) struct TableTextMetrics<'a> {
    pub(crate) text_system: &'a WindowTextSystem,
    pub(crate) text_sm: Pixels,
    pub(crate) text_xs: Pixels,
    pub(crate) cell_h_pad: Pixels,
    pub(crate) gap_1: Pixels,
    pub(crate) pl_5: Pixels,
    pub(crate) button_extra: Pixels,
    pub(crate) sort_icon_extra: Pixels,
    pub(crate) right_num_breathing: Pixels,
    pub(crate) sans_regular: Font,
    pub(crate) sans_medium: Font,
    pub(crate) sans_semibold: Font,
    pub(crate) mono_regular: Font,
    pub(crate) mono_semibold: Font,
}

impl<'a> TableTextMetrics<'a> {
    pub(crate) fn new(window: &'a Window, cx: &App) -> Self {
        let rem = window.rem_size();
        let pad = Size::Small.table_cell_padding();
        let theme = cx.theme();
        Self {
            text_system: window.text_system(),
            text_sm: rems(0.875).to_pixels(rem),
            text_xs: rems(0.75).to_pixels(rem),
            cell_h_pad: pad.left + pad.right,
            gap_1: rems(0.25).to_pixels(rem),
            pl_5: rems(1.25).to_pixels(rem),
            // `interactive_button` (`Button::small().compact()`): `px_1p5` (12px) +
            // `1px` border on each side + 4px breathing.
            button_extra: px(18.),
            // `DataTable` sort icon button (16px) + header `gap_2` (8px) + 2px slack.
            sort_icon_extra: px(26.),
            // Left clearance between right-aligned values and the `1px` column separator.
            right_num_breathing: px(10.),
            sans_regular: Font {
                family: theme.font_family.clone(),
                weight: FontWeight::NORMAL,
                ..Default::default()
            },
            sans_medium: Font {
                family: theme.font_family.clone(),
                weight: FontWeight::MEDIUM,
                ..Default::default()
            },
            sans_semibold: Font {
                family: theme.font_family.clone(),
                weight: FontWeight::SEMIBOLD,
                ..Default::default()
            },
            mono_regular: Font {
                family: theme.mono_font_family.clone(),
                weight: FontWeight::NORMAL,
                ..Default::default()
            },
            mono_semibold: Font {
                family: theme.mono_font_family.clone(),
                weight: FontWeight::SEMIBOLD,
                ..Default::default()
            },
        }
    }

    pub(crate) fn ceil_px(width: Pixels) -> Pixels {
        px(f32::from(width).ceil().max(24.))
    }

    pub(crate) fn header_width(&self, label: &str, sortable: bool) -> Pixels {
        let extra = if sortable {
            self.sort_icon_extra
        } else {
            px(4.)
        };
        Self::ceil_px(
            shape_text_width(
                self.text_system,
                SharedString::from(label.to_string()),
                self.sans_medium.clone(),
                self.text_sm,
            ) + self.cell_h_pad
                + extra,
        )
    }

    pub(crate) fn sans_cell(&self, text: SharedString, semibold: bool) -> Pixels {
        let font = if semibold {
            self.sans_semibold.clone()
        } else {
            self.sans_regular.clone()
        };
        Self::ceil_px(
            shape_text_width(self.text_system, text, font, self.text_sm) + self.cell_h_pad + px(4.),
        )
    }

    pub(crate) fn mono_cell(&self, text: SharedString, semibold: bool) -> Pixels {
        let font = if semibold {
            self.mono_semibold.clone()
        } else {
            self.mono_regular.clone()
        };
        Self::ceil_px(
            shape_text_width(self.text_system, text, font, self.text_sm) + self.cell_h_pad + px(4.),
        )
    }

    pub(crate) fn mono_right_cell(&self, text: SharedString) -> Pixels {
        Self::ceil_px(
            shape_text_width(
                self.text_system,
                text,
                self.mono_regular.clone(),
                self.text_sm,
            ) + self.cell_h_pad
                + self.right_num_breathing,
        )
    }

    pub(crate) fn mono_button_cell(&self, text: SharedString, semibold: bool) -> Pixels {
        let font = if semibold {
            self.mono_semibold.clone()
        } else {
            self.mono_regular.clone()
        };
        Self::ceil_px(
            shape_text_width(self.text_system, text, font, self.text_sm)
                + self.cell_h_pad
                + self.button_extra,
        )
    }

    pub(crate) fn grouped_mono_cell(&self, grouped: &GroupedText, window: &Window) -> Pixels {
        if grouped.as_str().is_empty() {
            return px(0.);
        }
        Self::ceil_px(
            grouped.measure_width(self.mono_regular.clone(), self.text_sm, window)
                + self.cell_h_pad
                + self.right_num_breathing,
        )
    }

    pub(crate) fn sans_width(&self, text: SharedString, semibold: bool) -> Pixels {
        let font = if semibold {
            self.sans_semibold.clone()
        } else {
            self.sans_regular.clone()
        };
        shape_text_width(self.text_system, text, font, self.text_sm)
    }

    pub(crate) fn mono_xs_width(&self, text: SharedString) -> Pixels {
        shape_text_width(
            self.text_system,
            text,
            self.mono_regular.clone(),
            self.text_xs,
        )
    }
}

fn is_fixed_content_column(col: &Column) -> bool {
    matches!(col.key.as_ref(), "date" | "flag") || matches!(col.name.as_ref(), "Date" | "Flag")
}

/// Scales `base` column widths to `target_width`:
/// - When `target_width <= sum(base)`, returns `base` unchanged so columns never
///   shrink below their content-fitted widths.
/// - When `target_width > sum(base)`, distributes the extra width proportionally
///   across all columns with monotonic cumulative rounding so `sum == target_width`
///   and every column is `>= base[i]`.
pub(crate) fn scale_widths_proportionally(base: &[Pixels], target_width: Pixels) -> Vec<Pixels> {
    let fixed = vec![false; base.len()];
    scale_widths_with_fixed(base, &fixed, target_width)
}

/// Scales `base` column widths to `target_width`, keeping any column where
/// `fixed[i]` is `true` (such as `"Date"` and `"Flag"`) pinned to `base[i]` and
/// distributing extra width proportionally across the remaining columns.
pub(crate) fn scale_widths_with_fixed(
    base: &[Pixels],
    fixed: &[bool],
    target_width: Pixels,
) -> Vec<Pixels> {
    if base.is_empty() {
        return Vec::new();
    }
    let base_sum: f32 = base.iter().map(|&w| f32::from(w)).sum();
    let target = f32::from(target_width);
    if base_sum <= 0.0 || target <= base_sum {
        return base.to_vec();
    }
    let scalable_indices: Vec<usize> = (0..base.len())
        .filter(|&ix| !fixed.get(ix).copied().unwrap_or(false))
        .collect();
    let scalable_base_sum: f32 = scalable_indices.iter().map(|&ix| f32::from(base[ix])).sum();
    if scalable_base_sum <= 0.0 {
        return base.to_vec();
    }
    let extra = target - base_sum;
    let k = scalable_indices.len();
    let mut out = base.to_vec();
    let mut cum_scalable = 0.0_f32;
    let mut prev_extra = 0.0_f32;
    for (pos, &col_ix) in scalable_indices.iter().enumerate() {
        cum_scalable += f32::from(base[col_ix]);
        let next_extra = if pos + 1 == k {
            extra
        } else {
            (extra * cum_scalable / scalable_base_sum).round()
        };
        let delta = (next_extra - prev_extra).max(0.0);
        out[col_ix] = base[col_ix] + px(delta);
        prev_extra = next_extra;
    }
    out
}

/// Column definitions for a full-width `DataTable` with no trailing empty column.
///
/// Each column tracks its content-fitted natural width (`natural_widths`). When
/// the table viewport is wider than the sum of column widths, fixed-content
/// columns (`"Date"` and `"Flag"`) keep their exact natural widths while all
/// other columns widen proportionally; when the viewport is narrower, columns
/// keep their natural widths so the table scrolls horizontally instead of
/// truncating cells.
pub(crate) struct TableColumns {
    columns: Vec<Column>,
    natural_widths: Vec<Pixels>,
    fixed: Vec<bool>,
    user_widths: Option<Vec<Pixels>>,
    table_width: Option<Pixels>,
}

impl TableColumns {
    const MAX_COL_WIDTH: Pixels = px(10_000.);

    pub(crate) fn new(columns: Vec<Column>) -> Self {
        let fixed: Vec<bool> = columns.iter().map(is_fixed_content_column).collect();
        let natural_widths = columns
            .iter()
            .map(|col| px(f32::from(col.width).ceil().max(24.)))
            .collect();
        let columns = columns
            .into_iter()
            .zip(&fixed)
            .map(|(col, &is_fixed)| {
                let col = col.min_width(px(20.)).max_width(Self::MAX_COL_WIDTH);
                if is_fixed { col.resizable(false) } else { col }
            })
            .collect();
        Self {
            columns,
            natural_widths,
            fixed,
            user_widths: None,
            table_width: None,
        }
    }

    pub(crate) fn as_slice(&self) -> &[Column] {
        &self.columns
    }

    pub(crate) fn len(&self) -> usize {
        self.columns.len()
    }

    pub(crate) fn get(&self, col_ix: usize) -> Option<&Column> {
        self.columns.get(col_ix)
    }

    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn natural_total_width(&self) -> Pixels {
        self.natural_widths.iter().copied().sum()
    }

    #[cfg(test)]
    pub(crate) fn natural_widths(&self) -> &[Pixels] {
        &self.natural_widths
    }

    pub(crate) fn move_column(&mut self, col_ix: usize, to_ix: usize) {
        if col_ix < self.columns.len() && to_ix < self.columns.len() && col_ix != to_ix {
            let col = self.columns.remove(col_ix);
            self.columns.insert(to_ix, col);
            let nat = self.natural_widths.remove(col_ix);
            self.natural_widths.insert(to_ix, nat);
            let is_fixed = self.fixed.remove(col_ix);
            self.fixed.insert(to_ix, is_fixed);
            if let Some(user) = &mut self.user_widths {
                let uw = user.remove(col_ix);
                user.insert(to_ix, uw);
            }
            self.table_width = None;
        }
    }

    pub(crate) fn set_sort(&mut self, col_ix: usize, sort: ColumnSort) {
        for (ix, col) in self.columns.iter_mut().enumerate() {
            if ix == col_ix {
                col.sort = Some(sort);
            } else if col.sort.is_some() {
                col.sort = Some(ColumnSort::Default);
            }
        }
    }

    /// Updates each column's content-fitted natural width (ordered to match
    /// `self.columns`) and re-applies proportional fitting if a viewport width
    /// is already known.
    pub(crate) fn set_natural_widths(&mut self, widths: Vec<Pixels>) -> bool {
        if widths.len() != self.columns.len() {
            return false;
        }
        let normalized: Vec<Pixels> = widths
            .into_iter()
            .map(|w| px(f32::from(w).ceil().max(24.)))
            .collect();
        if self.natural_widths == normalized {
            return false;
        }
        self.natural_widths = normalized;
        if let Some(user) = &mut self.user_widths {
            for ((uw, &nw), &is_fixed) in user.iter_mut().zip(&self.natural_widths).zip(&self.fixed)
            {
                if is_fixed || *uw < nw {
                    *uw = nw;
                }
            }
        }
        let prev_table_width = self.table_width.take();
        match prev_table_width {
            Some(width) => {
                self.fit_to_width(width);
                true
            }
            None => {
                let mut changed = false;
                for (col, &nw) in self.columns.iter_mut().zip(&self.natural_widths) {
                    if col.width != nw {
                        col.width = nw;
                        changed = true;
                    }
                }
                changed
            }
        }
    }

    /// Scales non-fixed columns proportionally to fill `width` when `width`
    /// exceeds the sum of base widths, keeping fixed columns (`"Date"` and
    /// `"Flag"`) at their content-fitted widths, or keeps base widths unchanged
    /// when `width` is narrower; returns `true` when any column width changed.
    pub(crate) fn fit_to_width(&mut self, width: Pixels) -> bool {
        if width <= px(0.) || self.table_width == Some(width) {
            return false;
        }
        self.table_width = Some(width);
        let base = self.user_widths.as_deref().unwrap_or(&self.natural_widths);
        let targets = scale_widths_with_fixed(base, &self.fixed, width);
        let mut changed = false;
        for (col, target) in self.columns.iter_mut().zip(targets) {
            if col.width != target {
                col.width = target;
                changed = true;
            }
        }
        changed
    }

    /// Records user-resized column widths and absorbs any sub-viewport slack
    /// into the neighboring scalable column so no empty trailing column gap opens.
    pub(crate) fn apply_resized_widths(&mut self, widths: &[Pixels]) -> bool {
        if widths.len() != self.columns.len() || self.columns.is_empty() {
            return false;
        }
        let resized_ix = self
            .columns
            .iter()
            .zip(widths)
            .position(|(col, &w)| col.width != w);
        let mut adjusted = widths.to_vec();
        for ((w, &nw), &is_fixed) in adjusted
            .iter_mut()
            .zip(&self.natural_widths)
            .zip(&self.fixed)
        {
            if is_fixed {
                *w = nw;
            }
        }
        let mut adjusted_slack = false;
        if let Some(table_width) = self.table_width {
            let total: Pixels = adjusted.iter().copied().sum();
            if total < table_width {
                let slack = table_width - total;
                let recipient = resized_ix
                    .and_then(|ix| (ix + 1..adjusted.len()).find(|&i| !self.fixed[i]))
                    .or_else(|| (0..adjusted.len()).rev().find(|&i| !self.fixed[i]));
                if let Some(recipient) = recipient {
                    adjusted[recipient] += slack;
                    adjusted_slack = true;
                }
            }
        }
        for (col, &w) in self.columns.iter_mut().zip(&adjusted) {
            col.width = w;
        }
        self.user_widths = Some(adjusted);
        adjusted_slack
    }
}

impl std::ops::Index<usize> for TableColumns {
    type Output = Column;

    fn index(&self, index: usize) -> &Self::Output {
        &self.columns[index]
    }
}

pub(crate) trait FullWidthDelegate: TableDelegate {
    fn columns_mut(&mut self) -> &mut TableColumns;

    fn sync_natural_widths(&mut self, _window: &Window, _cx: &App) -> bool {
        false
    }
}

/// Applies user-resized `widths` to `table` and refreshes its column groups if
/// slack adjustment was needed to keep the columns filling the viewport.
pub(crate) fn sync_resized_table_columns<D: FullWidthDelegate>(
    table: &Entity<TableState<D>>,
    widths: &[Pixels],
    cx: &mut App,
) {
    table.update(cx, |table, cx| {
        if table
            .delegate_mut()
            .columns_mut()
            .apply_resized_widths(widths)
        {
            table.refresh(cx);
            cx.notify();
        }
    });
}

/// Renders a small, borderless, unstriped [`DataTable`] whose columns fit their
/// contents and scale proportionally when the container is wider so no trailing
/// empty column gap appears.
pub(crate) fn full_width_data_table<D: FullWidthDelegate>(
    state: &Entity<TableState<D>>,
) -> FullWidthDataTable<D> {
    FullWidthDataTable {
        state: state.clone(),
    }
}

pub(crate) struct FullWidthDataTable<D: FullWidthDelegate> {
    state: Entity<TableState<D>>,
}

impl<D: FullWidthDelegate> IntoElement for FullWidthDataTable<D> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<D: FullWidthDelegate> Element for FullWidthDataTable<D> {
    type PrepaintState = AnyElement;
    type RequestLayoutState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let width = bounds.size.width;
        if width > px(0.) {
            self.state.update(cx, |table, cx| {
                let delegate = table.delegate_mut();
                let natural_changed = delegate.sync_natural_widths(window, cx);
                let fit_changed = delegate.columns_mut().fit_to_width(width);
                if natural_changed || fit_changed {
                    table.refresh(cx);
                }
            });
        }
        let mut element = DataTable::new(&self.state)
            .small()
            .bordered(false)
            .stripe(false)
            .into_any_element();
        element.prepaint_as_root(bounds.origin, bounds.size.into(), window, cx);
        element
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

/// Full-height `1px` left column separator for currency header cells in [`Table`].
pub(crate) fn trie_head_col_separator(
    #[cfg_attr(not(test), allow(unused_variables))] root: &'static str,
    #[cfg_attr(not(test), allow(unused_variables))] col_ix: usize,
    cx: &App,
) -> AnyElement {
    col_separator_line(
        px(0.),
        px(-1.),
        px(0.),
        || {
            #[cfg(test)]
            {
                (
                    ElementId::from("trie-col-sep-head"),
                    SharedString::from(format!("{root}:{col_ix}")),
                )
                    .into()
            }
            #[cfg(not(test))]
            {
                "trie-col-sep-head".into()
            }
        },
        cx,
    )
}

/// Full-height `1px` left column separator for currency data cells in [`Table`].
pub(crate) fn trie_cell_col_separator(
    #[cfg_attr(not(test), allow(unused_variables))] root: &'static str,
    row_ix: usize,
    #[cfg_attr(not(test), allow(unused_variables))] col_ix: usize,
    cx: &App,
) -> AnyElement {
    col_separator_line(
        if row_ix > 0 { px(-1.) } else { px(0.) },
        px(0.),
        px(0.),
        || {
            #[cfg(test)]
            {
                (
                    ElementId::from("trie-col-sep-cell"),
                    SharedString::from(format!("{root}:{row_ix}:{col_ix}")),
                )
                    .into()
            }
            #[cfg(not(test))]
            {
                "trie-col-sep-cell".into()
            }
        },
        cx,
    )
}

/// Background color for the user-focused record in report tables.
pub(crate) fn table_focused_row_bg(cx: &App) -> Hsla {
    cx.theme().table_active
}

/// A compact inline button colored to show that a datum in a table — a date
/// or an account name — can be clicked.
pub(crate) fn interactive_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let color = cx.theme().blue;
    let style = ButtonCustomVariant::new(cx)
        .foreground(color)
        .hover(color.opacity(0.12))
        .active(color.opacity(0.2));
    Button::new(id).custom(style).small().compact()
}

#[cfg(test)]
#[path = "style_test.rs"]
mod tests;

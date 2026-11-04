use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Size, StyledExt as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, Div, Element, ElementId, Entity, GlobalElementId, Hsla,
    InspectorElementId, InteractiveElement as _, IntoElement, LayoutId, ParentElement as _, Pixels,
    Stateful, Style, Styled as _, Window, div, px, relative,
};
#[cfg(test)]
use gpui_kit::{SharedString, TestSupportExt as _};

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

/// Column definitions for a full-width `DataTable` with no trailing empty column.
///
/// The column identified by `flex_key` expands whenever the table viewport is
/// wider than the sum of column widths, while manual column resizing keeps total
/// column width at least as wide as the viewport so no blank trailing gap opens.
pub(crate) struct TableColumns {
    columns: Vec<Column>,
    flex_key: &'static str,
    flex_base_width: Pixels,
    user_flex_width: Option<Pixels>,
    table_width: Option<Pixels>,
}

impl TableColumns {
    const MAX_COL_WIDTH: Pixels = px(10_000.);

    pub(crate) fn new(columns: Vec<Column>, flex_key: &'static str) -> Self {
        let mut flex_base_width = px(100.);
        let columns = columns
            .into_iter()
            .map(|col| {
                if col.key.as_ref() == flex_key {
                    flex_base_width = col.width;
                }
                col.max_width(Self::MAX_COL_WIDTH)
            })
            .collect();
        Self {
            columns,
            flex_key,
            flex_base_width,
            user_flex_width: None,
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

    pub(crate) fn move_column(&mut self, col_ix: usize, to_ix: usize) {
        if col_ix < self.columns.len() && to_ix < self.columns.len() && col_ix != to_ix {
            let col = self.columns.remove(col_ix);
            self.columns.insert(to_ix, col);
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

    /// Expands the flexible column to fill `width` when the viewport is wider
    /// than the remaining columns; returns `true` when any column width changed.
    pub(crate) fn fit_to_width(&mut self, width: Pixels) -> bool {
        if width <= px(0.) || self.table_width == Some(width) {
            return false;
        }
        self.table_width = Some(width);
        let other_width: Pixels = self
            .columns
            .iter()
            .filter(|col| col.key.as_ref() != self.flex_key)
            .map(|col| col.width)
            .sum();
        let min_flex = self.user_flex_width.unwrap_or(self.flex_base_width);
        let target_flex = (width - other_width).max(min_flex);
        if let Some(flex_col) = self
            .columns
            .iter_mut()
            .find(|col| col.key.as_ref() == self.flex_key)
            && flex_col.width != target_flex
        {
            flex_col.width = target_flex;
            return true;
        }
        false
    }

    /// Records user-resized column widths and absorbs any sub-viewport slack
    /// into the neighboring column so no empty trailing column gap opens.
    pub(crate) fn apply_resized_widths(&mut self, widths: &[Pixels]) -> bool {
        if widths.len() != self.columns.len() || self.columns.is_empty() {
            return false;
        }
        let resized_ix = self
            .columns
            .iter()
            .zip(widths)
            .position(|(col, &w)| col.width != w);
        for (col, &w) in self.columns.iter_mut().zip(widths) {
            col.width = w;
        }
        if let Some(ix) = resized_ix
            && self.columns[ix].key.as_ref() == self.flex_key
        {
            self.user_flex_width = Some(widths[ix]);
        }
        if let Some(table_width) = self.table_width {
            let total: Pixels = self.columns.iter().map(|col| col.width).sum();
            if total < table_width {
                let slack = table_width - total;
                let recipient = match resized_ix {
                    Some(ix) if ix + 1 < self.columns.len() => ix + 1,
                    _ => self.columns.len() - 1,
                };
                self.columns[recipient].width += slack;
                return true;
            }
        }
        false
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

/// Renders a small, borderless, unstriped [`DataTable`] whose flexible column
/// stretches to fill the container width before layout so no trailing empty
/// column gap appears.
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
                if table.delegate_mut().columns_mut().fit_to_width(width) {
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

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

#[cfg(test)]
use gpui_kit::TestSupportExt as _;
use gpui_kit::base::{POPUP_PRIORITY, Placement, Positioner};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, ClipboardItem, Element, ElementId, EntityId, Font, Global,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, InteractiveElement as _,
    IntoElement, LayoutId, MouseMoveEvent, ParentElement as _, Pixels, Point, ScrollEvent,
    SharedString, Styled as _, TextRun, Window, WindowId, WindowTextSystem, deferred, div, px,
};
use lumi::Amount;
use rust_decimal::Decimal;

/// Narrow horizontal gap between 3-digit integer groups (`2px`), visually
/// tighter than a full monospace space before currency codes like `USD`.
pub(crate) const DIGIT_GROUP_GAP: Pixels = px(2.);

#[derive(Clone, Debug, PartialEq, Eq)]
enum GroupedSpan {
    Number {
        num_ix: usize,
        raw: SharedString,
        groups: Arc<[SharedString]>,
        first_group_ix: usize,
    },
    Literal(SharedString),
}

/// Formatted text whose 3-digit integer groups render with [`DIGIT_GROUP_GAP`]
/// between groups and format with ASCII `' '` in plain text / accessibility labels.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GroupedText {
    plain: SharedString,
    ungrouped: SharedString,
    spans: Arc<[GroupedSpan]>,
    group_breaks: usize,
}

impl GroupedText {
    /// Formats a raw decimal string (such as `"1123456.7891"` or `"-2500.00"`).
    pub(crate) fn from_number_str(raw: &str) -> Self {
        if raw.is_empty() {
            return Self::default();
        }
        let mut builder = GroupedBuilder::new();
        builder.push_number_str(raw);
        builder.finish()
    }

    /// Formats `number` followed by a space and `currency`.
    pub(crate) fn from_decimal_currency(number: Decimal, currency: &str) -> Self {
        let mut builder = GroupedBuilder::new();
        builder.push_decimal(number);
        builder.push_str(" ");
        builder.push_str(currency);
        builder.finish()
    }

    /// Formats `amount` (`number currency`) with integer-part digit grouping.
    pub(crate) fn from_amount(amount: &Amount) -> Self {
        Self::from_decimal_currency(amount.number, &amount.currency)
    }

    pub(crate) fn as_str(&self) -> &str {
        self.plain.as_ref()
    }

    pub(crate) fn plain(&self) -> SharedString {
        self.plain.clone()
    }

    pub(crate) fn ungrouped(&self) -> &str {
        self.ungrouped.as_ref()
    }

    /// Number of narrow inter-group gaps rendered inside this text.
    pub(crate) fn group_breaks(&self) -> usize {
        self.group_breaks
    }

    /// Measures the exact rendered pixel width of this grouped text in `window`
    /// using `font` at `font_size`, including [`DIGIT_GROUP_GAP`] between groups.
    pub(crate) fn measure_width(&self, font: Font, font_size: Pixels, window: &Window) -> Pixels {
        if self.plain.is_empty() {
            return px(0.);
        }
        let text_system = window.text_system();
        if self.spans.is_empty() {
            return shape_text_width(text_system, self.plain.clone(), font, font_size);
        }
        let mut width = px(0.);
        for span in self.spans.iter() {
            match span {
                GroupedSpan::Number { groups, .. } => {
                    for part in groups.iter() {
                        width +=
                            shape_text_width(text_system, part.clone(), font.clone(), font_size);
                    }
                }
                GroupedSpan::Literal(text) => {
                    width += shape_text_width(text_system, text.clone(), font.clone(), font_size);
                }
            }
        }
        width + DIGIT_GROUP_GAP * self.group_breaks as f32
    }

    /// Renders the text inline, separating 3-digit integer groups by [`DIGIT_GROUP_GAP`]
    /// and attaching a hover menu with a copy button to each financial number.
    pub(crate) fn render(&self) -> AnyElement {
        if self.spans.is_empty() {
            return self.plain.clone().into_any_element();
        }
        if let [
            GroupedSpan::Number {
                num_ix,
                raw,
                groups,
                first_group_ix,
            },
        ] = self.spans.as_ref()
        {
            return FinancialNumberElement::new(
                *num_ix,
                raw.clone(),
                groups.clone(),
                *first_group_ix,
            )
            .into_any_element();
        }
        div()
            .h_flex()
            .items_center()
            .children(self.spans.iter().map(|span| {
                match span {
                    GroupedSpan::Number {
                        num_ix,
                        raw,
                        groups,
                        first_group_ix,
                    } => FinancialNumberElement::new(
                        *num_ix,
                        raw.clone(),
                        groups.clone(),
                        *first_group_ix,
                    )
                    .into_any_element(),
                    GroupedSpan::Literal(text) => text.clone().into_any_element(),
                }
            }))
            .into_any_element()
    }

    /// Renders right-aligned monospaced content for a `DataTable` cell.
    pub(crate) fn render_mono_right(&self, mono: SharedString) -> AnyElement {
        div()
            .h_flex()
            .justify_end()
            .font_family(mono)
            .child(self.render())
            .into_any_element()
    }
}

#[derive(Clone)]
struct ActiveNumberHover {
    target_id: GlobalElementId,
    raw: SharedString,
    trigger_bounds: Bounds<Pixels>,
    menu_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    copied: bool,
}

#[derive(Default)]
struct NumberHoverRegistry {
    windows: HashMap<WindowId, ActiveNumberHover>,
}

impl Global for NumberHoverRegistry {}

impl NumberHoverRegistry {
    fn same_trigger(a: Bounds<Pixels>, b: Bounds<Pixels>) -> bool {
        (a.origin.x - b.origin.x).abs() <= px(2.)
            && (a.origin.y - b.origin.y).abs() <= px(2.)
            && (a.size.width - b.size.width).abs() <= px(2.)
            && (a.size.height - b.size.height).abs() <= px(2.)
    }

    fn is_within_hover_zone(
        pos: Point<Pixels>,
        trigger: Bounds<Pixels>,
        menu: Option<Bounds<Pixels>>,
    ) -> bool {
        let pad = px(6.);
        let in_rect = |b: Bounds<Pixels>| {
            pos.x >= b.origin.x - pad
                && pos.x <= b.origin.x + b.size.width + pad
                && pos.y >= b.origin.y - pad
                && pos.y <= b.origin.y + b.size.height + pad
        };
        if in_rect(trigger) {
            return true;
        }
        let Some(menu) = menu else {
            return false;
        };
        if in_rect(menu) {
            return true;
        }
        let left = trigger.origin.x.min(menu.origin.x) - pad;
        let right =
            (trigger.origin.x + trigger.size.width).max(menu.origin.x + menu.size.width) + pad;
        let top = trigger.origin.y.min(menu.origin.y) - pad;
        let bottom =
            (trigger.origin.y + trigger.size.height).max(menu.origin.y + menu.size.height) + pad;
        pos.x >= left && pos.x <= right && pos.y >= top && pos.y <= bottom
    }

    fn is_active_id(
        window: &Window,
        target_id: &GlobalElementId,
        raw: &SharedString,
        cx: &App,
    ) -> bool {
        let window_id = window.window_handle().window_id();
        cx.try_global::<Self>()
            .and_then(|reg| reg.windows.get(&window_id))
            .is_some_and(|active| &active.target_id == target_id && &active.raw == raw)
    }

    fn active_for(
        window: &Window,
        target_id: &GlobalElementId,
        raw: &SharedString,
        bounds: Bounds<Pixels>,
        cx: &mut App,
    ) -> Option<ActiveNumberHover> {
        if !cx.has_global::<Self>() {
            return None;
        }
        let window_id = window.window_handle().window_id();
        let reg = cx.global_mut::<Self>();
        let active = reg.windows.get_mut(&window_id)?;
        if &active.target_id == target_id
            && &active.raw == raw
            && Self::same_trigger(active.trigger_bounds, bounds)
        {
            active.trigger_bounds = bounds;
            Some(active.clone())
        } else {
            None
        }
    }

    fn activate(
        window: &Window,
        target_id: GlobalElementId,
        raw: SharedString,
        bounds: Bounds<Pixels>,
        cx: &mut App,
    ) -> bool {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        let window_id = window.window_handle().window_id();
        let reg = cx.global_mut::<Self>();
        if let Some(existing) = reg.windows.get_mut(&window_id)
            && existing.target_id == target_id
            && existing.raw == raw
            && Self::same_trigger(existing.trigger_bounds, bounds)
        {
            existing.trigger_bounds = bounds;
            return false;
        }
        reg.windows.insert(
            window_id,
            ActiveNumberHover {
                target_id,
                raw,
                trigger_bounds: bounds,
                menu_bounds: Rc::new(Cell::new(None)),
                copied: false,
            },
        );
        true
    }

    fn should_deactivate_on_move(
        window: &Window,
        target_id: &GlobalElementId,
        pos: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut App,
    ) -> bool {
        if !cx.has_global::<Self>() {
            return false;
        }
        let window_id = window.window_handle().window_id();
        let reg = cx.global_mut::<Self>();
        let Some(active) = reg.windows.get(&window_id) else {
            return false;
        };
        if &active.target_id != target_id || !Self::same_trigger(active.trigger_bounds, bounds) {
            return false;
        }
        if Self::is_within_hover_zone(pos, bounds, active.menu_bounds.get()) {
            return false;
        }
        reg.windows.remove(&window_id);
        true
    }

    fn mark_copied(window: &Window, target_id: &GlobalElementId, cx: &mut App) {
        if !cx.has_global::<Self>() {
            return;
        }
        let window_id = window.window_handle().window_id();
        if let Some(active) = cx.global_mut::<Self>().windows.get_mut(&window_id)
            && &active.target_id == target_id
        {
            active.copied = true;
        }
    }

    fn deactivate_target(window: &Window, target_id: &GlobalElementId, cx: &mut App) -> bool {
        if !cx.has_global::<Self>() {
            return false;
        }
        let window_id = window.window_handle().window_id();
        let reg = cx.global_mut::<Self>();
        if reg
            .windows
            .get(&window_id)
            .is_some_and(|active| &active.target_id == target_id)
        {
            reg.windows.remove(&window_id);
            true
        } else {
            false
        }
    }
}

fn render_number_hover_menu(
    target_id: GlobalElementId,
    raw: SharedString,
    copied: bool,
    view: EntityId,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let (icon, label) = if copied {
        (IconName::Check, "Copied")
    } else {
        (IconName::Copy, "Copy number")
    };
    let button = Button::new("copy-number-button")
        .ghost()
        .xsmall()
        .icon(icon)
        .label(label)
        .accessibility_label("Copy number")
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            cx.write_to_clipboard(ClipboardItem::new_string(raw.to_string()));
            NumberHoverRegistry::mark_copied(window, &target_id, cx);
            cx.notify(view);
            window.refresh();
        });

    let menu = div()
        .id("number-hover-menu")
        .occlude()
        .h_flex()
        .items_center()
        .p(px(2.))
        .rounded(theme.radius)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .font_family(theme.font_family.clone())
        .font_normal()
        .text_sm()
        .border_1()
        .border_color(theme.border)
        .shadow_sm()
        .child(button);
    #[cfg(test)]
    let menu = menu.test_support();
    menu
}

struct FinancialNumberElement {
    num_ix: usize,
    raw: SharedString,
    groups: Arc<[SharedString]>,
    first_group_ix: usize,
}

impl FinancialNumberElement {
    fn new(
        num_ix: usize,
        raw: SharedString,
        groups: Arc<[SharedString]>,
        first_group_ix: usize,
    ) -> Self {
        Self {
            num_ix,
            raw,
            groups,
            first_group_ix,
        }
    }
}

impl IntoElement for FinancialNumberElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FinancialNumberElement {
    type PrepaintState = Hitbox;
    type RequestLayoutState = AnyElement;

    fn id(&self) -> Option<ElementId> {
        Some(("financial-number-host", self.num_ix).into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let is_active = global_id
            .is_some_and(|gid| NumberHoverRegistry::is_active_id(window, gid, &self.raw, cx));
        let highlight_bg = {
            let theme = cx.theme();
            theme
                .primary
                .opacity(if theme.is_dark() { 0.22 } else { 0.14 })
        };
        let mut inner = if self.groups.len() <= 1 {
            let text = self
                .groups
                .first()
                .cloned()
                .unwrap_or_else(|| self.raw.clone());
            let el = div()
                .id(("financial-number", self.num_ix))
                .rounded(px(3.))
                .when(is_active, |this| this.bg(highlight_bg))
                .child(text);
            #[cfg(test)]
            let el = el.test_support();
            el.into_any_element()
        } else {
            let first_group_ix = self.first_group_ix;
            let el = div()
                .id(("financial-number", self.num_ix))
                .rounded(px(3.))
                .when(is_active, |this| this.bg(highlight_bg))
                .h_flex()
                .items_center()
                .gap(DIGIT_GROUP_GAP)
                .children(self.groups.iter().enumerate().map(|(ix, part)| {
                    #[cfg(test)]
                    {
                        div()
                            .id(("digit-group", first_group_ix + ix))
                            .test_support()
                            .child(part.clone())
                            .into_any_element()
                    }
                    #[cfg(not(test))]
                    {
                        let _ = (first_group_ix, ix);
                        part.clone().into_any_element()
                    }
                }));
            #[cfg(test)]
            let el = el.test_support();
            el.into_any_element()
        };
        let layout_id = inner.request_layout(window, cx);
        (layout_id, inner)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        inner: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        inner.prepaint(window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        if let Some(global_id) = global_id
            && let Some(active) =
                NumberHoverRegistry::active_for(window, global_id, &self.raw, bounds, cx)
        {
            let menu_bounds = active.menu_bounds.clone();
            let menu = render_number_hover_menu(
                global_id.clone(),
                self.raw.clone(),
                active.copied,
                window.current_view(),
                cx,
            );
            let mut overlay = deferred(
                Positioner::side(bounds)
                    .placement(Placement::Top)
                    .offset(px(2.))
                    .margin(px(6.))
                    .occlude()
                    .on_position(move |resolved| {
                        menu_bounds.set(Some(resolved.bounds));
                    })
                    .child(menu),
            )
            .with_priority(POPUP_PRIORITY)
            .into_any_element();
            overlay.prepaint_as_root(Point::default(), window.viewport_size().into(), window, cx);
        }

        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        inner: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        inner.paint(window, cx);
        let Some(global_id) = global_id.cloned() else {
            return;
        };
        let hitbox = hitbox.clone();
        let raw = self.raw.clone();
        let view = window.current_view();
        let gid_on_move = global_id.clone();
        window.on_mouse_event(
            move |event: &MouseMoveEvent, _, window: &mut Window, cx: &mut App| {
                if hitbox.is_hovered(window) {
                    if NumberHoverRegistry::activate(
                        window,
                        gid_on_move.clone(),
                        raw.clone(),
                        bounds,
                        cx,
                    ) {
                        cx.notify(view);
                        window.refresh();
                    }
                } else if NumberHoverRegistry::should_deactivate_on_move(
                    window,
                    &gid_on_move,
                    event.position,
                    bounds,
                    cx,
                ) {
                    cx.notify(view);
                    window.refresh();
                }
            },
        );
        window.on_mouse_event(
            move |_: &ScrollEvent, _, window: &mut Window, cx: &mut App| {
                if NumberHoverRegistry::deactivate_target(window, &global_id, cx) {
                    cx.notify(view);
                    window.refresh();
                }
            },
        );
    }
}

impl AsRef<str> for GroupedText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for GroupedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub(crate) fn shape_text_width(
    text_system: &WindowTextSystem,
    text: SharedString,
    font: Font,
    font_size: Pixels,
) -> Pixels {
    if text.is_empty() {
        return px(0.);
    }
    if text.contains(['\n', '\r']) {
        return text
            .lines()
            .map(|line| {
                if line.is_empty() {
                    px(0.)
                } else {
                    let line_str = SharedString::from(line.to_string());
                    text_system
                        .shape_line(
                            line_str.clone(),
                            font_size,
                            &[TextRun {
                                len: line_str.len(),
                                font: font.clone(),
                                color: Hsla::default(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                        )
                        .width
                }
            })
            .fold(px(0.), Pixels::max);
    }
    text_system
        .shape_line(
            text.clone(),
            font_size,
            &[TextRun {
                len: text.len(),
                font,
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        )
        .width
}

/// Incrementally builds a [`GroupedText`] from numbers and literal text (such
/// as currency codes, dates, braces, or commas) so only numeric integer parts
/// are grouped.
#[derive(Default)]
pub(crate) struct GroupedBuilder {
    plain: String,
    ungrouped: String,
    spans: Vec<GroupedSpan>,
    num_count: usize,
    next_group_ix: usize,
    group_breaks: usize,
}

impl GroupedBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Appends literal non-grouped text (e.g. `" USD"`, `", 2024-03-01}"`).
    pub(crate) fn push_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.plain.push_str(text);
        self.ungrouped.push_str(text);
        if let Some(GroupedSpan::Literal(prev)) = self.spans.last_mut() {
            *prev = SharedString::from(format!("{prev}{text}"));
        } else {
            self.spans
                .push(GroupedSpan::Literal(SharedString::from(text.to_string())));
        }
    }

    /// Appends `number` with 3-digit grouping on its integer part only.
    pub(crate) fn push_decimal(&mut self, number: Decimal) {
        self.push_number_str(&number.to_string());
    }

    /// Appends `amount` (`number currency`) with 3-digit grouping on `amount.number`.
    pub(crate) fn push_amount(&mut self, amount: &Amount) {
        self.push_decimal(amount.number);
        self.push_str(" ");
        self.push_str(&amount.currency);
    }

    /// Appends a decimal string (optional leading `-`/`+`, integer digits, and
    /// optional `.fraction`), inserting group breaks every 3 integer digits.
    pub(crate) fn push_number_str(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let (sign, unsigned) = match raw.as_bytes().first() {
            Some(b'-' | b'+') => (&raw[..1], &raw[1..]),
            _ => ("", raw),
        };
        let (int_part, frac_suffix) = match unsigned.find('.') {
            Some(dot) => (&unsigned[..dot], &unsigned[dot..]),
            None => (unsigned, ""),
        };
        if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
            self.push_str(raw);
            return;
        }
        self.ungrouped.push_str(raw);

        let mut groups = Vec::new();
        if int_part.len() <= 3 {
            self.plain.push_str(raw);
            groups.push(SharedString::from(raw.to_string()));
        } else {
            let first_len = match int_part.len() % 3 {
                0 => 3,
                rem => rem,
            };
            let mut group = String::with_capacity(sign.len() + first_len);
            group.push_str(sign);
            group.push_str(&int_part[..first_len]);
            self.plain.push_str(&group);
            groups.push(SharedString::from(group));

            let mut rest = &int_part[first_len..];
            while !rest.is_empty() {
                self.plain.push(' ');
                let is_last = rest.len() == 3;
                let mut part =
                    String::with_capacity(3 + if is_last { frac_suffix.len() } else { 0 });
                part.push_str(&rest[..3]);
                if is_last {
                    part.push_str(frac_suffix);
                }
                self.plain.push_str(&part);
                groups.push(SharedString::from(part));
                rest = &rest[3..];
            }
            self.group_breaks += groups.len() - 1;
        }

        let num_ix = self.num_count;
        self.num_count += 1;
        let first_group_ix = self.next_group_ix;
        if groups.len() > 1 {
            self.next_group_ix += groups.len();
        }
        self.spans.push(GroupedSpan::Number {
            num_ix,
            raw: SharedString::from(raw.to_string()),
            groups: Arc::from(groups),
            first_group_ix,
        });
    }

    pub(crate) fn finish(self) -> GroupedText {
        GroupedText {
            plain: self.plain.into(),
            ungrouped: self.ungrouped.into(),
            spans: Arc::from(self.spans),
            group_breaks: self.group_breaks,
        }
    }
}

#[cfg(test)]
#[path = "number_test.rs"]
mod tests;

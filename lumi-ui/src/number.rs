use std::fmt;
use std::sync::Arc;

use gpui_kit::component::StyledExt as _;
use gpui_kit::{
    AnyElement, Font, Hsla, IntoElement, ParentElement as _, Pixels, SharedString, Styled as _,
    TextRun, Window, WindowTextSystem, div, px,
};
#[cfg(test)]
use gpui_kit::{InteractiveElement as _, TestSupportExt as _};
use lumi::Amount;
use rust_decimal::Decimal;

/// Narrow horizontal gap between 3-digit integer groups (`2px`), visually
/// tighter than a full monospace space before currency codes like `USD`.
pub(crate) const DIGIT_GROUP_GAP: Pixels = px(2.);

/// Formatted text whose 3-digit integer groups render with [`DIGIT_GROUP_GAP`]
/// between groups and format with ASCII `' '` in plain text / accessibility labels.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GroupedText {
    plain: SharedString,
    ungrouped: SharedString,
    parts: Arc<[SharedString]>,
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
        self.parts.len().saturating_sub(1)
    }

    /// Measures the exact rendered pixel width of this grouped text in `window`
    /// using `font` at `font_size`, including [`DIGIT_GROUP_GAP`] between groups.
    pub(crate) fn measure_width(&self, font: Font, font_size: Pixels, window: &Window) -> Pixels {
        if self.plain.is_empty() {
            return px(0.);
        }
        let text_system = window.text_system();
        if self.parts.is_empty() {
            return shape_text_width(text_system, self.plain.clone(), font, font_size);
        }
        let parts_width = self
            .parts
            .iter()
            .map(|part| shape_text_width(text_system, part.clone(), font.clone(), font_size))
            .fold(px(0.), |acc, w| acc + w);
        parts_width + DIGIT_GROUP_GAP * self.group_breaks() as f32
    }

    /// Renders the text inline, separating 3-digit integer groups by [`DIGIT_GROUP_GAP`].
    pub(crate) fn render(&self) -> AnyElement {
        if self.parts.is_empty() {
            return self.plain.clone().into_any_element();
        }
        div()
            .h_flex()
            .items_center()
            .gap(DIGIT_GROUP_GAP)
            .children(self.parts.iter().enumerate().map(|(ix, part)| {
                #[cfg(test)]
                {
                    div()
                        .id(("digit-group", ix))
                        .test_support()
                        .child(part.clone())
                        .into_any_element()
                }
                #[cfg(not(test))]
                {
                    let _ = ix;
                    part.clone().into_any_element()
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
    ungrouped: String,
    segments: Vec<String>,
    current: String,
}

impl GroupedBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Appends literal non-grouped text (e.g. `" USD"`, `", 2024-03-01}"`).
    pub(crate) fn push_str(&mut self, text: &str) {
        self.ungrouped.push_str(text);
        self.current.push_str(text);
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
        if int_part.len() <= 3 {
            self.current.push_str(raw);
            return;
        }

        let first_len = match int_part.len() % 3 {
            0 => 3,
            rem => rem,
        };
        self.current.push_str(sign);
        self.current.push_str(&int_part[..first_len]);

        let mut rest = &int_part[first_len..];
        while !rest.is_empty() {
            let prev = std::mem::take(&mut self.current);
            self.segments.push(prev);
            self.current.push_str(&rest[..3]);
            rest = &rest[3..];
        }
        self.current.push_str(frac_suffix);
    }

    pub(crate) fn finish(mut self) -> GroupedText {
        if self.segments.is_empty() {
            let plain: SharedString = self.ungrouped.into();
            return GroupedText {
                ungrouped: plain.clone(),
                plain,
                parts: Arc::from([]),
            };
        }
        self.segments.push(self.current);
        let plain: SharedString = self.segments.join(" ").into();
        let ungrouped: SharedString = self.ungrouped.into();
        let parts: Arc<[SharedString]> =
            self.segments.into_iter().map(SharedString::from).collect();
        GroupedText {
            plain,
            ungrouped,
            parts,
        }
    }
}

#[cfg(test)]
#[path = "number_test.rs"]
mod tests;

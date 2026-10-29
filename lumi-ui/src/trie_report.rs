use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, ElementId, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window, div, rems,
};
use lumi::web::{TrieOptions, TrieTable};

use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::style::interactive_button;

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
    _subscriptions: Vec<Subscription>,
}

/// The ledger state and options a set of tables reflects.
#[derive(Clone, Copy, PartialEq, Eq)]
struct LoadKey {
    revision: u64,
    show_closed: bool,
}

enum TrieLoad {
    Pending,
    Ready(TriePresentation),
    /// No account starts with the root.
    Missing,
    Failed(SharedString),
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
    /// The last segment of the account name.
    name: SharedString,
    /// One formatted balance per currency; empty for a zero balance.
    numbers: Vec<SharedString>,
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

    /// Queries the tables for the current ledger revision and options, unless
    /// that exact request has already been made.
    fn load(&mut self, cx: &mut Context<Self>) {
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

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.requested = None;
        self.load(cx);
    }

    fn open_account(&mut self, account: SharedString, cx: &mut Context<Self>) {
        cx.emit(ReportEvent::OpenAccount(account));
    }

    fn render_table(
        &self,
        root: &'static str,
        table: &TrieLoad,
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
                let theme = cx.theme();
                let mono = theme.mono_font_family.clone();
                let radius = theme.radius;
                let border = theme.border;
                let head_bg = theme.secondary;
                let head_fg = theme.secondary_foreground;
                let even_bg = theme.table;
                let odd_bg = if theme.is_dark() {
                    theme.muted.opacity(0.45)
                } else {
                    theme.muted
                };
                div()
                    .rounded(radius)
                    .border_1()
                    .border_color(border)
                    .shadow_sm()
                    .child(
                        Table::new()
                            .small()
                            .rounded(radius)
                            .accessibility_label(root)
                            .child(
                                TableHeader::new()
                                    .bg(head_bg)
                                    .text_color(head_fg)
                                    .font_medium()
                                    .child(
                                        TableRow::new()
                                            .child(TableHead::new().min_w_40().child("Account"))
                                            .children(presentation.currencies.iter().map(
                                                |currency| {
                                                    TableHead::new()
                                                        .w_24()
                                                        .text_right()
                                                        .child(currency.clone())
                                                },
                                            )),
                                    ),
                            )
                            .child(TableBody::new().border_t_1().border_color(border).children(
                                presentation.rows.iter().enumerate().map(|(row_ix, row)| {
                                    let account = row.account.clone();
                                    let row_bg = if row_ix.is_multiple_of(2) {
                                        even_bg
                                    } else {
                                        odd_bg
                                    };
                                    TableRow::new()
                                        .bg(row_bg)
                                        .child(
                                            TableCell::new().child(
                                                div().pl(rems(row.level as f32 * 1.25)).child(
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
                                                    .accessibility_label(row.account.clone())
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.open_account(account.clone(), cx)
                                                    })),
                                                ),
                                            ),
                                        )
                                        .children(row.numbers.iter().map(|number| {
                                            TableCell::new()
                                                .w_24()
                                                .text_right()
                                                .font_family(mono.clone())
                                                .when(row.level == 0, |this| this.font_semibold())
                                                .child(number.clone())
                                        }))
                                }),
                            )),
                    )
                    .into_any_element()
            }
        }
    }
}

impl TriePresentation {
    /// Flattens the rows of `table`, resolving each row's full account name
    /// from the names of the rows above it.
    fn new(root: &str, table: TrieTable<String>) -> Self {
        let mut ancestors: Vec<(String, usize)> = Vec::new();
        let rows: Vec<TrieRow> = table
            .rows
            .into_iter()
            .map(|row| {
                while ancestors
                    .last()
                    .is_some_and(|(_, level)| *level >= row.level)
                {
                    ancestors.pop();
                }
                ancestors.push((row.name.clone(), row.level));
                let account = ancestors
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(":");
                TrieRow {
                    account: account.into(),
                    level: row.level,
                    name: row.name.into(),
                    numbers: row.numbers.into_iter().map(SharedString::from).collect(),
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

impl EventEmitter<ReportEvent> for TrieReport {}

impl Render for TrieReport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page.clone();
        let mut tables = self.tables.iter();
        div()
            .v_flex()
            .size_full()
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
                // Tables take the width their columns need and the report
                // scrolls both ways, so an amount is never clipped.
                div()
                    .id((ElementId::from("trie-report"), page.id()))
                    .flex_1()
                    .min_h_0()
                    // A two-way scroll area lays this out as an item of a flex
                    // row, which would stretch it to the area's height and so
                    // leave nothing to scroll to vertically.
                    .self_start()
                    .overflow_scrollbar()
                    .child(div().h_flex().items_start().gap_6().p_6().children(
                        columns(&page).iter().map(|column| {
                            div()
                                .v_flex()
                                .flex_none()
                                .gap_6()
                                .children(column.iter().map(|root| {
                                    let table = tables.next().unwrap_or(&TrieLoad::Pending);
                                    // `Table` has no id of its own, so without
                                    // this scope sibling tables would share
                                    // element paths and so accessibility node
                                    // ids.
                                    div()
                                        .id((ElementId::from("trie-table"), *root))
                                        .child(self.render_table(root, table, cx))
                                }))
                        }),
                    )),
            )
    }
}

#[cfg(test)]
#[path = "trie_report_test.rs"]
mod tests;

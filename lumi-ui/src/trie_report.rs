use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, ElementId, Entity, EventEmitter, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Task,
    TestSupportExt as _, Window, div, px, relative, rems,
};
use lumi::web::{TrieOptions, TrieTable};

use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::style::{
    interactive_button, table_card, table_row_bg, trie_cell_col_separator, trie_head_col_separator,
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
    numbers: Vec<SharedString>,
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
                let border = theme.border;
                let guide_color = muted.opacity(0.55);
                table_card(cx)
                    .child(
                        Table::new()
                            .small()
                            .accessibility_label(root)
                            .child(
                                TableHeader::new().font_medium().child(
                                    TableRow::new()
                                        .child(TableHead::new().min_w_40().child("Account"))
                                        .children(presentation.currencies.iter().enumerate().map(
                                            |(col_ix, currency)| {
                                                TableHead::new()
                                                    .relative()
                                                    .w_24()
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
                                                    div().py(px(3.)).flex().items_center().child(
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
                                                TableCell::new()
                                                    .relative()
                                                    .w_24()
                                                    .text_right()
                                                    .font_family(mono.clone())
                                                    .when(row.level == 0, |this| {
                                                        this.font_semibold()
                                                    })
                                                    .child(number.clone())
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
                        .cloned()
                        .map(SharedString::from)
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

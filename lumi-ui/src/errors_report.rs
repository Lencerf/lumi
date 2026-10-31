use std::sync::Arc;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::Button;
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, EventEmitter, IntoElement, ParentElement as _,
    Render, SharedString, Stateful, Styled as _, Subscription, Task, WeakEntity, Window, div, px,
};
use lumi::ErrorLevel;

use crate::navigation::ReportEvent;
use crate::session::LedgerSession;
use crate::source::LedgerSource;
use crate::style::{
    data_table_td, data_table_th, interactive_button, table_card, table_header, table_row,
    table_row_bg,
};

/// The errors, warnings and notes the last reload of the ledger reported,
/// most severe first.
pub struct ErrorsReport {
    session: Entity<LedgerSession>,
    table: Entity<TableState<ErrorsDelegate>>,
    requested_revision: Option<u64>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ErrorRow {
    level: ErrorLevel,
    message: SharedString,
    file: SharedString,
    line: usize,
    /// `file:line:column` of the directive the error is about.
    location: SharedString,
}

struct ErrorsDelegate {
    source: Arc<dyn LedgerSource>,
    report: WeakEntity<ErrorsReport>,
    columns: Vec<Column>,
    errors: Vec<ErrorRow>,
    loading: bool,
    failed: Option<SharedString>,
    generation: u64,
    _load_task: Option<Task<()>>,
}

impl EventEmitter<ReportEvent> for ErrorsReport {}

impl ErrorsReport {
    pub fn new(
        session: Entity<LedgerSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = session.read(cx).source().clone();
        let report = cx.entity().downgrade();
        let table = cx.new(|cx| TableState::new(ErrorsDelegate::new(source, report), window, cx));
        let _subscriptions = vec![cx.observe(&session, |this, _, cx| this.load(cx))];
        let mut this = Self {
            session,
            table,
            requested_revision: None,
            _subscriptions,
        };
        this.load(cx);
        this
    }

    /// The levels and messages of the rows shown, in table order.
    pub fn messages(&self, cx: &App) -> Vec<(ErrorLevel, SharedString)> {
        self.table
            .read(cx)
            .delegate()
            .errors
            .iter()
            .map(|error| (error.level, error.message.clone()))
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
}

impl Render for ErrorsReport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (summary, failed) = {
            let delegate = self.table.read(cx).delegate();
            (format_summary(&delegate.errors), delegate.failed.clone())
        };
        div()
            .v_flex()
            .size_full()
            .min_h_0()
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
                            Alert::error("errors-failed", message)
                                .title("Couldn't read the errors"),
                        )
                        .child(
                            Button::new("errors-retry")
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
                            .stripe(false),
                    ),
                ),
            )
    }
}

fn format_summary(rows: &[ErrorRow]) -> SharedString {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    for row in rows {
        match row.level {
            ErrorLevel::Error => errors += 1,
            ErrorLevel::Warning => warnings += 1,
            ErrorLevel::Info => infos += 1,
        }
    }
    let mut parts = Vec::with_capacity(3);
    match errors {
        0 => parts.push("No errors".to_string()),
        1 => parts.push("1 error".to_string()),
        n => parts.push(format!("{n} errors")),
    }
    if warnings > 0 {
        parts.push(match warnings {
            1 => "1 warning".to_string(),
            n => format!("{n} warnings"),
        });
    }
    if infos > 0 {
        parts.push(format!("{infos} info"));
    }
    parts.join(", ").into()
}

fn level_text(level: ErrorLevel) -> &'static str {
    match level {
        ErrorLevel::Error => "Error",
        ErrorLevel::Warning => "Warning",
        ErrorLevel::Info => "Info",
    }
}

impl ErrorsDelegate {
    fn new(source: Arc<dyn LedgerSource>, report: WeakEntity<ErrorsReport>) -> Self {
        Self {
            source,
            report,
            columns: vec![
                Column::new("level", "Level").width(px(90.)),
                Column::new("message", "Message").width(px(620.)),
                Column::new("location", "Location").width(px(300.)),
            ],
            errors: Vec::new(),
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
        let task = self.source.errors(cx);
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
                        Ok(errors) => delegate.errors = error_rows(errors),
                        Err(error) => delegate.failed = Some(format!("{error:#}").into()),
                    }
                    cx.notify();
                })
                .ok();
        }));
        cx.notify();
    }
}

/// The rows for `errors`: most severe first, then by location. Locations
/// are shown relative to the directory all the files share, which for a
/// ledger is the directory of its main file.
fn error_rows(mut errors: Vec<lumi::Error>) -> Vec<ErrorRow> {
    errors.sort_by(|a, b| {
        b.level
            .cmp(&a.level)
            .then_with(|| a.src.file.cmp(&b.src.file))
            .then_with(|| a.src.start.cmp(&b.src.start))
    });
    let common_dir = common_dir(errors.iter().map(|error| error.src.file.as_str()));
    errors
        .into_iter()
        .map(|error| {
            let file = error
                .src
                .file
                .strip_prefix(&common_dir)
                .unwrap_or(&error.src.file);
            ErrorRow {
                level: error.level,
                message: error.msg.into(),
                file: error.src.file.as_str().into(),
                line: error.src.start.line,
                location: format!("{file}:{}:{}", error.src.start.line, error.src.start.col).into(),
            }
        })
        .collect()
}

/// The longest directory prefix (ending in `/`) shared by all `paths`; empty
/// if there is none.
pub(crate) fn common_dir<'a>(mut paths: impl Iterator<Item = &'a str>) -> String {
    /// `path` up to and including its last `/`.
    fn dir_of(path: &str) -> &str {
        &path[..path.rfind('/').map_or(0, |ix| ix + 1)]
    }
    let Some(first) = paths.next() else {
        return String::new();
    };
    let mut prefix = dir_of(first);
    for path in paths {
        while !prefix.is_empty() && !path.starts_with(prefix) {
            prefix = dir_of(&prefix[..prefix.len() - 1]);
        }
    }
    prefix.to_string()
}

impl TableDelegate for ErrorsDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.errors.len()
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

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        data_table_th(&self.columns, col_ix, cx)
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let row = table_row(row_ix);
        if row_ix < self.errors.len() {
            row.bg(table_row_bg(row_ix, cx))
        } else {
            row
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let error = &self.errors[row_ix];
        let columns_count = self.columns.len();
        let cell = match self.columns[col_ix].key.as_ref() {
            "level" => div()
                .text_color(match error.level {
                    ErrorLevel::Error => cx.theme().red,
                    ErrorLevel::Warning => cx.theme().yellow,
                    ErrorLevel::Info => cx.theme().blue,
                })
                .when(
                    matches!(error.level, ErrorLevel::Error | ErrorLevel::Warning),
                    |this| this.font_semibold(),
                )
                .child(level_text(error.level)),
            "message" => div().child(error.message.clone()),
            "location" => {
                let report = self.report.clone();
                let file = error.file.clone();
                let line = error.line;
                let location = error.location.clone();
                div().h_flex().child(
                    interactive_button(("error-location", row_ix), cx)
                        .font_family(cx.theme().mono_font_family.clone())
                        .label(location.clone())
                        .accessibility_label(format!("Open {location} in editor"))
                        .tooltip("Open in editor")
                        .on_click(move |_, _, cx| {
                            report
                                .update(cx, |_, cx| {
                                    cx.emit(ReportEvent::OpenSource {
                                        file: file.clone(),
                                        line,
                                    });
                                })
                                .ok();
                        }),
                )
            }
            _ => div(),
        };
        data_table_td(row_ix, col_ix, columns_count, cell, cx)
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, _: &App) -> Option<SharedString> {
        let error = self.errors.get(row_ix)?;
        Some(match self.columns.get(col_ix)?.key.as_ref() {
            "level" => level_text(error.level).into(),
            "message" => error.message.clone(),
            "location" => error.location.clone(),
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
            .child("No errors")
    }

    fn loading(&self, _: &App) -> bool {
        self.loading
    }
}

#[cfg(test)]
#[path = "errors_report_test.rs"]
mod tests;

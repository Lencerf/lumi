use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::assets::IconName as AssetIconName;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    Editor, EditorState, Enter, Escape, Input, InputEdit, InputEvent, InputState, Position,
    RopeExt as _, Search,
};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, IndexPath, Selectable as _, Sizable as _,
    StyledExt as _, h_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, AvailableSpace, Bounds, ContentMask, Context, Element,
    ElementId, Entity, Focusable as _, GlobalElementId, InspectorElementId,
    InteractiveElement as _, IntoElement, LayoutId, MouseDownEvent, MouseMoveEvent,
    ParentElement as _, Pixels, Point, Render, ScrollEvent, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TestSupportExt as _,
    WeakEntity, Window, actions, canvas, deferred, div, fill, point, px, size,
};
use lumi::ErrorLevel;

use crate::align::PostingAligner;
use crate::completion::BeancountCompletionProvider;
use crate::errors_report::common_dir;
use crate::input_guard::EditDragGuard;
use crate::session::LedgerSession;
use crate::style::{editor_bg, editor_error_line_bg, table_card};
use crate::{completion, highlight};

actions!(lumi_ui, [SaveSource]);

/// The key context of the full-file source editor; its key bindings apply
/// within it.
pub(crate) const KEY_CONTEXT: &str = "SourceEditor";

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceFileItem {
    path: SharedString,
    label: SharedString,
}

impl SelectItem for SourceFileItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.path
    }

    fn matches(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.label.to_lowercase().contains(&q) || self.path.to_lowercase().contains(&q)
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .font_family(cx.theme().mono_font_family.clone())
            .child(self.label.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ErrorLineEntry {
    /// 0-based line index in the buffer (`error.src.start.line - 1`).
    row: usize,
    /// Error messages reported on this line, in column order.
    messages: Vec<SharedString>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HoveredError {
    row: usize,
    anchor_x: Pixels,
}

#[derive(Default)]
struct ActiveFileErrors {
    original: Option<SharedString>,
    ledger_entries: Vec<ErrorLineEntry>,
    entries: Vec<ErrorLineEntry>,
}

#[cfg(test)]
type PaintedErrorLines = Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>>;

struct Draft {
    original: SharedString,
    text: SharedString,
    error_lines: Vec<ErrorLineEntry>,
}

/// Built-in multi-file Beancount source editor, modeled on Fava's Editor view.
///
/// A dropdown at the top lists the root ledger file and all included source
/// files; the editor below provides Beancount syntax highlighting, line
/// numbers, in-file search, and auto-completion for accounts, currencies,
/// payees, narrations, tags, and links.
pub struct SourceEditor {
    session: Entity<LedgerSession>,
    file_select: Entity<SelectState<SearchableVec<SourceFileItem>>>,
    text: Entity<EditorState>,
    drag_guard: EditDragGuard,
    aligner: PostingAligner,
    search_input: Entity<InputState>,
    search_drag_guard: EditDragGuard,
    search_open: bool,
    search_case_insensitive: bool,
    provider: Rc<BeancountCompletionProvider>,
    files: Vec<SharedString>,
    selected_file: Option<SharedString>,
    /// Text of `selected_file` as last read from or written to disk.
    original: Option<SharedString>,
    /// Unsaved buffers for non-active files when switching files in the dropdown.
    drafts: HashMap<SharedString, Draft>,
    /// `ErrorLevel::Error` diagnostics from the last loaded ledger, grouped by
    /// source file path and starting line.
    ledger_errors: HashMap<SharedString, Vec<ErrorLineEntry>>,
    /// Active error lines for `selected_file`, shifted across unsaved buffer edits.
    active_errors: Rc<RefCell<ActiveFileErrors>>,
    /// Error line currently under the mouse pointer, if any.
    hovered_error: Option<HoveredError>,
    #[cfg(test)]
    painted_error_lines: PaintedErrorLines,
    /// 1-based line number to place the cursor on once `selected_file` loads.
    pending_line: Option<usize>,
    loading_file: bool,
    saving: bool,
    error: Option<SharedString>,
    requested_revision: Option<u64>,
    meta_generation: u64,
    file_generation: u64,
    _meta_task: Option<Task<()>>,
    _file_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl SourceEditor {
    pub fn new(
        session: Entity<LedgerSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let provider = BeancountCompletionProvider::new();
        let provider_for_state = provider.clone();
        let active_errors = Rc::new(RefCell::new(ActiveFileErrors::default()));
        let active_errors_for_edits = active_errors.clone();
        // Pass `.searchable(false)` (`EditorState::new` enables `searchable` by
        // default) so `gpui-component`'s built-in `SearchPanel` overlay—which
        // re-selects the query when `anchor_offset` transitions from `None` to
        // `Some(...)` on the first typed character—never mounts; `SourceEditor`
        // renders its own search bar and drives `EditorState`'s search session
        // directly.
        let text = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language(highlight::LANGUAGE)
                .searchable(false);
            state.set_soft_wrap(false, window, cx);
            state.set_line_number(true, window, cx);
            state.set_folding(false, window, cx);
            state.set_scroll_beyond_last_line(Some(0), window, cx);
            state.set_highlighter_factory(
                highlight::highlighter_factory_with_edit_listener(Rc::new(move |edit, text| {
                    let mut active = active_errors_for_edits.borrow_mut();
                    apply_edit_to_error_lines(&mut active.entries, edit);
                    if active
                        .original
                        .as_ref()
                        .is_some_and(|orig| orig.len() == text.len() && text == orig.as_ref())
                    {
                        let lines_len = text.lines_len();
                        active.entries = active
                            .ledger_entries
                            .iter()
                            .filter(|entry| entry.row < lines_len)
                            .cloned()
                            .collect();
                    }
                })),
                cx,
            );
            let lsp = state.lsp_mut();
            lsp.completion_provider = Some(provider_for_state);
            lsp.completion_menu.max_width = px(480.);
            lsp.set_completion_highlighter(completion::highlighter());
            state
        });
        let drag_guard = EditDragGuard::new(&text);
        let aligner = PostingAligner::new(&text, true, cx);
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in file…"));
        let search_drag_guard = EditDragGuard::new(&search_input);
        let file_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let _subscriptions = vec![
            cx.observe_in(&session, window, |this, _, window, cx| {
                this.on_session_changed(window, cx)
            }),
            cx.subscribe_in(&file_select, window, Self::on_select_event),
            cx.subscribe(&text, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.hovered_error = None;
                    this.drag_guard.cancel();
                    if !this.is_changed(cx)
                        && let Some(path) = this.selected_file.clone()
                    {
                        this.reset_active_errors_from_ledger(&path, cx);
                    }
                    cx.notify();
                }
            }),
            cx.subscribe_in(&text, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.aligner
                        .on_change(&this.text, &this.provider, window, cx);
                }
            }),
            cx.subscribe(&search_input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.search_drag_guard.cancel();
                    this.sync_search_query(true, cx);
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            session,
            file_select,
            text,
            drag_guard,
            aligner,
            search_input,
            search_drag_guard,
            search_open: false,
            search_case_insensitive: true,
            provider,
            files: Vec::new(),
            selected_file: None,
            original: None,
            drafts: HashMap::new(),
            ledger_errors: HashMap::new(),
            active_errors,
            hovered_error: None,
            #[cfg(test)]
            painted_error_lines: Rc::new(RefCell::new(Vec::new())),
            pending_line: None,
            loading_file: false,
            saving: false,
            error: None,
            requested_revision: None,
            meta_generation: 0,
            file_generation: 0,
            _meta_task: None,
            _file_task: None,
            _subscriptions,
        };
        this.on_session_changed(window, cx);
        this
    }

    /// All source files currently available in the dropdown, in display order.
    pub fn files(&self) -> &[SharedString] {
        &self.files
    }

    /// The path of the currently selected source file.
    pub fn selected_file(&self) -> Option<&SharedString> {
        self.selected_file.as_ref()
    }

    /// The current editor buffer contents.
    pub fn value(&self, cx: &App) -> SharedString {
        self.text.read(cx).value()
    }

    /// The 1-based line number of the active cursor in the editor.
    pub fn cursor_line(&self, cx: &App) -> usize {
        self.text.read(cx).cursor_position().line as usize + 1
    }

    /// Whether the active file has unsaved modifications.
    pub fn is_changed(&self, cx: &App) -> bool {
        self.original
            .as_ref()
            .is_some_and(|original| *original != self.text.read(cx).value())
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub fn is_search_open(&self) -> bool {
        self.search_open
    }

    pub fn is_search_focused(&self, window: &Window, cx: &App) -> bool {
        self.search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }

    pub fn search_query(&self, cx: &App) -> SharedString {
        self.search_input.read(cx).value()
    }

    /// 1-based line numbers and error messages currently highlighted in the
    /// active source file.
    pub fn error_lines(&self, cx: &App) -> Vec<(usize, Vec<SharedString>)> {
        let lines_len = self.text.read(cx).text().lines_len();
        self.active_errors
            .borrow()
            .entries
            .iter()
            .filter(|entry| entry.row < lines_len)
            .map(|entry| (entry.row + 1, entry.messages.clone()))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn editor_state(&self) -> &Entity<EditorState> {
        &self.text
    }

    #[cfg(test)]
    pub(crate) fn search_input_state(&self) -> &Entity<InputState> {
        &self.search_input
    }

    #[cfg(test)]
    pub(crate) fn painted_error_lines(&self) -> Vec<(usize, Bounds<Pixels>)> {
        self.painted_error_lines.borrow().clone()
    }

    /// Opens the in-file search bar, focuses its input, and selects any
    /// existing or editor-selected query text.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = true;
        self.search_drag_guard.cancel();
        let selected = self.text.read(cx).selected_value();
        let seed = (!selected.is_empty() && !selected.contains('\n') && !selected.contains('\r'))
            .then_some(selected);
        self.search_input.update(cx, |input, cx| {
            if let Some(seed) = seed {
                input.set_value(seed, window, cx);
            }
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.sync_search_query(true, cx);
        cx.notify();
    }

    fn sync_search_query(&mut self, reveal_match: bool, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value();
        let case_insensitive = self.search_case_insensitive;
        self.text.update(cx, |state, cx| {
            state.set_search_query(query.to_string(), case_insensitive, cx);
            if reveal_match {
                let matcher = &state.search_session().matcher;
                if let Some(ix) = matcher.current()
                    && let Some(range) = matcher.matched_ranges().get(ix).cloned()
                {
                    let match_row = state.text().offset_to_position(range.start).line as usize;
                    let in_view = state
                        .visible_row_range()
                        .is_none_or(|visible| visible.contains(&match_row));
                    if !in_view {
                        scroll_line_to_top(state, match_row as u32, cx);
                    }
                }
            }
        });
    }

    pub fn next_search_match(&mut self, cx: &mut Context<Self>) {
        self.text.update(cx, |state, cx| {
            state.next_search_match(cx);
        });
        cx.notify();
    }

    pub fn previous_search_match(&mut self, cx: &mut Context<Self>) {
        self.text.update(cx, |state, cx| {
            state.previous_search_match(cx);
        });
        cx.notify();
    }

    fn toggle_search_case_sensitive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_case_insensitive = !self.search_case_insensitive;
        self.sync_search_query(true, cx);
        self.search_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = false;
        self.search_drag_guard.cancel();
        self.text.update(cx, |state, cx| {
            state.close_search(cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    /// Selects the source file at `path` and loads its text into the editor.
    pub fn select_file(
        &mut self,
        path: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path: SharedString = path.into();
        if !self.files.contains(&path) {
            return;
        }
        self.pending_line = None;
        self.file_select.update(cx, |select, cx| {
            select.set_selected_value(&path, window, cx)
        });
        self.switch_to_file(path, true, window, cx);
    }

    /// Selects `path` in the source editor and moves the cursor to the start of
    /// 1-based `line`, scrolling that line into view once the file is loaded.
    pub fn open_location(
        &mut self,
        path: impl Into<SharedString>,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path: SharedString = path.into();
        if self.files.is_empty() {
            // The initial `source.files()` query is still in flight; record the
            // target file and line so `on_session_changed` selects and jumps to
            // it as soon as the file list and text arrive.
            self.selected_file = Some(path);
            self.pending_line = Some(line);
            return;
        }
        if !self.files.contains(&path) {
            return;
        }
        self.pending_line = Some(line);
        self.file_select.update(cx, |select, cx| {
            select.set_selected_value(&path, window, cx)
        });
        self.switch_to_file(path, true, window, cx);
    }

    fn on_select_event(
        &mut self,
        _: &Entity<SelectState<SearchableVec<SourceFileItem>>>,
        event: &SelectEvent<SearchableVec<SourceFileItem>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(Some(path)) = event else {
            return;
        };
        self.pending_line = None;
        self.switch_to_file(path.clone(), true, window, cx);
    }

    fn switch_to_file(
        &mut self,
        path: SharedString,
        focus_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_file.as_ref() == Some(&path) {
            if !self.loading_file {
                if let Some(line) = self.pending_line.take() {
                    move_cursor_to_line(&self.text, line, window, cx);
                } else if focus_editor {
                    self.text.update(cx, |state, cx| state.focus(window, cx));
                }
            }
            return;
        }

        self.hovered_error = None;

        // Preserve any unsaved edits on the file we are leaving.
        if let (Some(prev_path), Some(orig)) = (&self.selected_file, &self.original) {
            let current = self.text.read(cx).value();
            if current != *orig {
                self.drafts.insert(
                    prev_path.clone(),
                    Draft {
                        original: orig.clone(),
                        text: current,
                        error_lines: self.active_errors.borrow().entries.clone(),
                    },
                );
            } else {
                self.drafts.remove(prev_path);
            }
        }

        self.selected_file = Some(path.clone());
        self.error = None;

        if let Some(draft) = self.drafts.remove(&path) {
            self.file_generation += 1;
            self._file_task = None;
            self.loading_file = false;
            self.original = Some(draft.original.clone());
            self.text
                .update(cx, |state, cx| state.set_value(draft.text, window, cx));
            {
                let mut active = self.active_errors.borrow_mut();
                active.original = Some(draft.original);
                active.ledger_entries = self.ledger_errors.get(&path).cloned().unwrap_or_default();
                active.entries = draft.error_lines;
            }
            if self.search_open {
                self.sync_search_query(false, cx);
            }
            if let Some(line) = self.pending_line.take() {
                move_cursor_to_line(&self.text, line, window, cx);
            } else if focus_editor {
                self.text.update(cx, |state, cx| state.focus(window, cx));
            }
            cx.notify();
            return;
        }

        self.original = None;
        *self.active_errors.borrow_mut() = ActiveFileErrors::default();
        self.load_file_text(path, focus_editor, window, cx);
    }

    fn reset_active_errors_from_ledger(&self, path: &SharedString, cx: &App) {
        let lines_len = self.text.read(cx).text().lines_len();
        let ledger_entries = self.ledger_errors.get(path).cloned().unwrap_or_default();
        let entries = ledger_entries
            .iter()
            .filter(|entry| entry.row < lines_len)
            .cloned()
            .collect();
        let mut active = self.active_errors.borrow_mut();
        active.original = self.original.clone();
        active.ledger_entries = ledger_entries;
        active.entries = entries;
    }

    fn clear_hovered_error(&mut self, cx: &mut Context<Self>) {
        if self.hovered_error.take().is_some() {
            cx.notify();
        }
    }

    fn on_editor_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.original.is_none() || self.saving || event.pressed_button.is_some() {
            self.clear_hovered_error(cx);
            return;
        }
        let matched_row = {
            let state = self.text.read(cx);
            editor_painted_viewport_geometry(state).and_then(
                |(input_bounds, line_height, scroll_offset)| {
                    if event.position.x < input_bounds.left()
                        || event.position.x >= input_bounds.right()
                        || event.position.y < input_bounds.top()
                        || event.position.y >= input_bounds.bottom()
                    {
                        return None;
                    }
                    let origin_y = input_bounds.origin.y + scroll_offset.y;
                    let rel_y = event.position.y - origin_y;
                    if rel_y < px(0.) {
                        return None;
                    }
                    let row = (rel_y / line_height).floor() as usize;
                    (row < state.text().lines_len()
                        && self
                            .active_errors
                            .borrow()
                            .entries
                            .binary_search_by_key(&row, |entry| entry.row)
                            .is_ok())
                    .then_some(row)
                },
            )
        };
        match matched_row {
            Some(row) => {
                if self
                    .hovered_error
                    .as_ref()
                    .is_none_or(|hovered| hovered.row != row)
                {
                    self.hovered_error = Some(HoveredError {
                        row,
                        anchor_x: event.position.x,
                    });
                    cx.notify();
                }
            }
            None => self.clear_hovered_error(cx),
        }
    }

    fn on_session_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.meta_generation += 1;
        let generation = self.meta_generation;
        let files_task = source.files(cx);
        let completions_task = source.completions(cx);
        let errors_task = source.errors(cx);
        self._meta_task = Some(cx.spawn_in(window, async move |this, cx| {
            let (files_res, completions_res, errors_res) =
                (files_task.await, completions_task.await, errors_task.await);
            this.update_in(cx, |this, window, cx| {
                if this.meta_generation != generation {
                    return;
                }
                if let Ok(completions) = completions_res {
                    this.provider.set_completions(completions);
                }
                if let Ok(errors) = errors_res {
                    this.ledger_errors = group_errors_by_file(errors);
                }
                match files_res {
                    Ok(raw_files) => {
                        let items = build_file_items(&raw_files);
                        this.files = items.iter().map(|item| item.path.clone()).collect();
                        let requested = this.selected_file.clone();
                        let target = requested
                            .as_ref()
                            .filter(|selected| this.files.contains(selected))
                            .cloned()
                            .or_else(|| this.files.first().cloned());
                        if target != requested {
                            this.pending_line = None;
                        }
                        let selected_ix = target
                            .as_ref()
                            .and_then(|t| this.files.iter().position(|f| f == t))
                            .map(IndexPath::new);
                        this.file_select.update(cx, |select, cx| {
                            select.set_items(SearchableVec::new(items), window, cx);
                            select.set_selected_index(selected_ix, window, cx);
                        });
                        if let Some(target) = target {
                            let same_file = this.selected_file.as_ref() == Some(&target);
                            this.selected_file = Some(target.clone());
                            // Reload from disk unless the user currently has unsaved edits.
                            if !same_file || !this.is_changed(cx) {
                                if !same_file {
                                    this.original = None;
                                    *this.active_errors.borrow_mut() = ActiveFileErrors::default();
                                } else if this.original.is_some() {
                                    this.reset_active_errors_from_ledger(&target, cx);
                                }
                                this.hovered_error = None;
                                this.load_file_text(target, false, window, cx);
                            }
                        } else {
                            this.selected_file = None;
                            this.original = None;
                            *this.active_errors.borrow_mut() = ActiveFileErrors::default();
                            this.hovered_error = None;
                            this.pending_line = None;
                        }
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn load_file_text(
        &mut self,
        path: SharedString,
        focus_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.loading_file = true;
        self.file_generation += 1;
        let generation = self.file_generation;
        let source = self.session.read(cx).source().clone();
        let task = source.file_text(&path, cx);
        self._file_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                if this.file_generation != generation {
                    return;
                }
                this.loading_file = false;
                match result {
                    Ok(content) => {
                        let already_matches = this.original.is_some()
                            && this.text.read(cx).value().as_ref() == content.as_str();
                        this.original = Some(content.clone().into());
                        if !already_matches {
                            this.text
                                .update(cx, |state, cx| state.set_value(content, window, cx));
                            if this.search_open {
                                this.sync_search_query(false, cx);
                            }
                        }
                        this.hovered_error = None;
                        this.reset_active_errors_from_ledger(&path, cx);
                        if let Some(line) = this.pending_line.take() {
                            move_cursor_to_line(&this.text, line, window, cx);
                        } else if focus_editor {
                            this.text.update(cx, |state, cx| state.focus(window, cx));
                        }
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Discards unsaved changes in the active file, restoring the last saved text.
    pub fn revert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.loading_file || !self.is_changed(cx) {
            return;
        }
        let Some(original) = self.original.clone() else {
            return;
        };
        if let Some(path) = &self.selected_file {
            self.drafts.remove(path);
        }
        self.error = None;
        self.hovered_error = None;
        self.text.update(cx, |state, cx| {
            state.set_value(original, window, cx);
            state.focus(window, cx);
        });
        if let Some(path) = self.selected_file.clone() {
            self.reset_active_errors_from_ledger(&path, cx);
        }
        if self.search_open {
            self.sync_search_query(false, cx);
        }
        cx.notify();
    }

    /// Writes the active file to disk and reloads the ledger so reports and
    /// completion candidates reflect the saved changes.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.loading_file || !self.is_changed(cx) {
            return;
        }
        let (Some(path), Some(original)) = (self.selected_file.clone(), self.original.clone())
        else {
            return;
        };
        let new_text = self.text.read(cx).value();
        let source = self.session.read(cx).source().clone();
        let task = source.replace_file_text(&path, original.to_string(), new_text.to_string(), cx);
        self.saving = true;
        self.error = None;
        cx.notify();
        self._file_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, _, cx| {
                this.saving = false;
                match result {
                    Ok(()) => {
                        this.drafts.remove(&path);
                        if this.selected_file.as_ref() == Some(&path) {
                            this.original = Some(new_text.clone());
                            this.active_errors.borrow_mut().original = Some(new_text);
                        }
                        this.session.update(cx, |session, cx| session.reload(cx));
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn on_save(&mut self, _: &SaveSource, window: &mut Window, cx: &mut Context<Self>) {
        self.save(window, cx);
    }
}

fn build_file_items(paths: &[String]) -> Vec<SourceFileItem> {
    let prefix = common_dir(paths.iter().map(String::as_str));
    paths
        .iter()
        .map(|path| {
            let short = path.strip_prefix(&prefix).unwrap_or(path);
            let label = if short.is_empty() {
                path.as_str()
            } else {
                short
            };
            SourceFileItem {
                path: path.clone().into(),
                label: label.to_string().into(),
            }
        })
        .collect()
}

impl Render for SourceEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let loaded = self.original.is_some();
        let changed = self.is_changed(cx);
        let saving = self.saving;
        let busy = self.loading_file || saving;
        let has_files = !self.files.is_empty();
        let selected_path = self.selected_file.clone();
        let error = self.error.clone();
        let search_open = self.search_open;
        let case_sensitive = !self.search_case_insensitive;
        let has_query = !self.search_input.read(cx).value().is_empty();
        let (has_matches, match_label): (bool, SharedString) = {
            let matcher = &self.text.read(cx).search_session().matcher;
            (!matcher.is_empty(), matcher.label().into())
        };

        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(|this, _: &Search, window, cx| this.focus_search(window, cx)))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.search_open {
                    this.close_search(window, cx);
                }
            }))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_6()
                    .pt_4()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div().w(px(280.)).flex_shrink_0().child(
                                    Select::new(&self.file_select)
                                        .id("source-file-select")
                                        .small()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .placeholder("Select source file…")
                                        .accessibility_label("Source file")
                                        .disabled(!has_files || saving),
                                ),
                            )
                            .when_some(selected_path, |this, path| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(path),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .flex_shrink_0()
                            .when(changed, |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().yellow)
                                        .child("Unsaved changes"),
                                )
                            })
                            .child(
                                Button::new("revert-source")
                                    .outline()
                                    .small()
                                    .label("Revert")
                                    .disabled(!changed || busy)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.revert(window, cx)),
                                    ),
                            )
                            .child(
                                Button::new("save-source")
                                    .primary()
                                    .small()
                                    .label("Save")
                                    .loading(saving)
                                    .disabled(!changed || busy)
                                    .tooltip_with_action(
                                        "Save file and reload ledger",
                                        &SaveSource,
                                        Some(KEY_CONTEXT),
                                    )
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.save(window, cx)),
                                    ),
                            ),
                    ),
            )
            .when_some(error, |this, message| {
                this.child(
                    div()
                        .px_6()
                        .pt_3()
                        .child(Alert::error("source-editor-error", message)),
                )
            })
            .child(
                div().flex_1().min_h_0().p_6().pt_3().child(
                    table_card(cx)
                        .v_flex()
                        .relative()
                        .size_full()
                        .min_h_0()
                        .when(search_open, |this| {
                            this.child(
                                h_flex()
                                    .id("source-editor-search-bar")
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .px_3()
                                    .py_1p5()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().tokens.popover)
                                    .on_action(cx.listener(|this, action: &Enter, _, cx| {
                                        if action.shift {
                                            this.previous_search_match(cx);
                                        } else {
                                            this.next_search_match(cx);
                                        }
                                    }))
                                    .on_action(cx.listener(|this, _: &Escape, window, cx| {
                                        this.close_search(window, cx);
                                    }))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .relative()
                                                    .w(px(320.))
                                                    .max_w_full()
                                                    .capture_action(
                                                        self.search_drag_guard.paste_capture(),
                                                    )
                                                    .child(self.search_drag_guard.element())
                                                    .child(
                                                        Input::new(&self.search_input)
                                                            .id("source-editor-search")
                                                            .small()
                                                            .cleanable(true)
                                                            .aria_label("Find in file")
                                                            .prefix(
                                                                Icon::new(IconName::Search)
                                                                    .small()
                                                                    .text_color(
                                                                        cx.theme().muted_foreground,
                                                                    ),
                                                            )
                                                            .suffix(
                                                                Button::new("source-search-case")
                                                                    .xsmall()
                                                                    .compact()
                                                                    .ghost()
                                                                    .icon(IconName::CaseSensitive)
                                                                    .selected(case_sensitive)
                                                                    .tooltip("Match case")
                                                                    .on_click(cx.listener(
                                                                        |this, _, window, cx| {
                                                                            this.toggle_search_case_sensitive(window, cx);
                                                                        },
                                                                    )),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                Button::new("source-search-prev")
                                                    .xsmall()
                                                    .ghost()
                                                    .icon(IconName::ChevronLeft)
                                                    .tooltip("Previous match (Shift+Enter)")
                                                    .disabled(!has_matches)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.previous_search_match(cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new("source-search-next")
                                                    .xsmall()
                                                    .ghost()
                                                    .icon(IconName::ChevronRight)
                                                    .tooltip("Next match (Enter)")
                                                    .disabled(!has_matches)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.next_search_match(cx);
                                                    })),
                                            )
                                            .when(has_query, |this| {
                                                this.child(
                                                    div()
                                                        .text_xs()
                                                        .min_w(px(48.))
                                                        .text_color(if has_matches {
                                                            cx.theme().foreground
                                                        } else {
                                                            cx.theme().muted_foreground
                                                        })
                                                        .child(match_label),
                                                )
                                            }),
                                    )
                                    .child(
                                        Button::new("source-search-close")
                                            .xsmall()
                                            .ghost()
                                            .icon(IconName::Close)
                                            .tooltip("Close search (Escape)")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_search(window, cx);
                                            })),
                                    ),
                            )
                        })
                        .child({
                            let editor_bg = editor_bg(cx);
                            let error_line_bg = editor_error_line_bg(cx);
                            let text_for_backdrop = self.text.clone();
                            let errors_for_backdrop = self.active_errors.clone();
                            #[cfg(test)]
                            let painted_for_backdrop = self.painted_error_lines.clone();
                            let hovered_popover = self
                                .hovered_error
                                .filter(|_| loaded && !saving)
                                .and_then(|hovered| {
                                    let lines_len = self.text.read(cx).text().lines_len();
                                    if hovered.row >= lines_len {
                                        return None;
                                    }
                                    let active = self.active_errors.borrow();
                                    let ix = active
                                        .entries
                                        .binary_search_by_key(&hovered.row, |e| e.row)
                                        .ok()?;
                                    Some(ErrorHoverPopover {
                                        editor: self.text.clone(),
                                        source_editor: cx.entity().downgrade(),
                                        row: hovered.row,
                                        anchor_x: hovered.anchor_x,
                                        messages: active.entries[ix].messages.clone(),
                                    })
                                });

                            div()
                                .id("source-editor-body")
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .bg(editor_bg)
                                .on_mouse_move(cx.listener(Self::on_editor_mouse_move))
                                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                    if !*hovered {
                                        this.clear_hovered_error(cx);
                                    }
                                }))
                                .capture_action(self.drag_guard.paste_capture())
                                .capture_action(self.aligner.undo_capture())
                                .capture_action(self.aligner.redo_capture())
                                .child(self.drag_guard.element())
                                .when(loaded, |this| {
                                    this.child(
                                        canvas(
                                            |_, _, _| (),
                                            move |backdrop_bounds, _, window, cx| {
                                                paint_error_line_backgrounds(
                                                    backdrop_bounds,
                                                    &text_for_backdrop,
                                                    &errors_for_backdrop,
                                                    error_line_bg,
                                                    #[cfg(test)]
                                                    &painted_for_backdrop,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )
                                        .absolute()
                                        .inset_0(),
                                    )
                                })
                                .child(
                                    Editor::new(&self.text)
                                        .size_full()
                                        .appearance(false)
                                        .bordered(false)
                                        .aria_label("Source text")
                                        .disabled(!loaded || saving),
                                )
                                .when_some(hovered_popover, |this, popover| this.child(popover))
                                .when(!loaded && self.error.is_none(), |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(Spinner::new()),
                                    )
                                })
                        }),
                ),
            )
    }
}

fn group_errors_by_file(
    mut errors: Vec<lumi::Error>,
) -> HashMap<SharedString, Vec<ErrorLineEntry>> {
    errors.retain(|error| error.level == ErrorLevel::Error && error.src.start.line >= 1);
    errors.sort_by(|a, b| {
        a.src
            .file
            .cmp(&b.src.file)
            .then_with(|| a.src.start.line.cmp(&b.src.start.line))
            .then_with(|| a.src.start.col.cmp(&b.src.start.col))
    });
    let mut by_file: HashMap<SharedString, Vec<ErrorLineEntry>> = HashMap::new();
    for error in errors {
        let row = error.src.start.line - 1;
        let msg: SharedString = error.msg.into();
        let entries = by_file.entry(error.src.file.as_str().into()).or_default();
        if let Some(last) = entries.last_mut().filter(|last| last.row == row) {
            if !last.messages.contains(&msg) {
                last.messages.push(msg);
            }
        } else {
            entries.push(ErrorLineEntry {
                row,
                messages: vec![msg],
            });
        }
    }
    by_file
}

fn apply_edit_to_error_lines(entries: &mut Vec<ErrorLineEntry>, edit: &InputEdit) {
    if edit.start_byte == edit.old_end_byte && edit.start_byte == edit.new_end_byte {
        return;
    }
    let mut updated: Vec<ErrorLineEntry> = Vec::with_capacity(entries.len());
    for entry in entries.drain(..) {
        let Some(next_row) = shift_error_row(entry.row, edit) else {
            continue;
        };
        if let Some(prev) = updated.last_mut().filter(|prev| prev.row == next_row) {
            for msg in entry.messages {
                if !prev.messages.contains(&msg) {
                    prev.messages.push(msg);
                }
            }
        } else {
            updated.push(ErrorLineEntry {
                row: next_row,
                messages: entry.messages,
            });
        }
    }
    *entries = updated;
}

fn shift_error_row(row: usize, edit: &InputEdit) -> Option<usize> {
    let old_start = edit.start_position.row;
    let old_end = edit.old_end_position.row;
    let new_end = edit.new_end_position.row;
    if row < old_start {
        Some(row)
    } else if row > old_end {
        Some((row as isize + (new_end as isize - old_end as isize)) as usize)
    } else if old_start == old_end {
        if new_end > old_end && edit.start_position.column == 0 {
            Some(row + (new_end - old_end))
        } else {
            Some(row)
        }
    } else if row == old_start && edit.start_position.column > 0 {
        Some(old_start)
    } else if row == old_end && edit.old_end_position.column == 0 {
        Some(new_end)
    } else {
        None
    }
}

/// Viewport geometry during `prepaint` / `paint` of the current frame, after
/// `Editor`'s `prepaint` has updated `prepaint_caret_geometry`.
fn editor_render_viewport_geometry(
    state: &EditorState,
) -> Option<(Bounds<Pixels>, Pixels, Point<Pixels>)> {
    if let Some(geom) = state.prepaint_caret_geometry() {
        let input_bounds = geom.input_bounds();
        let line_height = geom.line_height();
        if input_bounds.size.height > px(0.) && line_height > px(0.) {
            // `PrepaintCaretGeometry::scroll_offset` is clamped to the scroll
            // range and omits macOS elastic overscroll (`stretch`), whereas
            // `cursor_bounds` is laid out from `input_bounds.top() + stretch`
            // before `scroll_offset.y` is applied. Recover `stretch` from the
            // active caret so error backgrounds and hovers move with elastic
            // bounce at the top and bottom of the file.
            let cursor_bounds = geom.cursor_bounds();
            let cursor_row = state
                .display_map()
                .buffer_line_to_display_row(state.cursor_position().line as usize);
            let cursor_line_top =
                cursor_bounds.origin.y - (line_height - cursor_bounds.size.height) * 0.5;
            let stretch_y =
                cursor_line_top - input_bounds.origin.y - line_height * cursor_row as f32;
            let mut scroll_offset = geom.scroll_offset();
            scroll_offset.y += stretch_y;
            return Some((input_bounds, line_height, scroll_offset));
        }
    }
    editor_painted_viewport_geometry(state)
}

/// Viewport geometry from the most recently painted frame (`text_bounds`
/// includes both clamped scroll and any active elastic overscroll stretch),
/// used for mouse hit-testing between frames.
fn editor_painted_viewport_geometry(
    state: &EditorState,
) -> Option<(Bounds<Pixels>, Pixels, Point<Pixels>)> {
    let line_height = state.line_height().filter(|&h| h > px(0.))?;
    let input_bounds = state.input_bounds();
    if input_bounds.size.height <= px(0.) {
        return None;
    }
    let text_bounds = state.text_bounds()?;
    let scroll_offset = point(
        text_bounds.origin.x - input_bounds.origin.x,
        text_bounds.origin.y - input_bounds.origin.y,
    );
    Some((input_bounds, line_height, scroll_offset))
}

#[allow(clippy::too_many_arguments)]
fn paint_error_line_backgrounds(
    backdrop_bounds: Bounds<Pixels>,
    editor: &Entity<EditorState>,
    active_errors: &Rc<RefCell<ActiveFileErrors>>,
    error_bg: gpui_kit::Hsla,
    #[cfg(test)] painted_out: &PaintedErrorLines,
    window: &mut Window,
    cx: &mut App,
) {
    #[cfg(test)]
    painted_out.borrow_mut().clear();

    let active = active_errors.borrow();
    let entries = &active.entries;
    if entries.is_empty() {
        return;
    }
    let state = editor.read(cx);
    let Some((input_bounds, line_height, scroll_offset)) = editor_render_viewport_geometry(state)
    else {
        return;
    };
    let lines_len = state.text().lines_len();
    if lines_len == 0 {
        return;
    }

    let clip_top = input_bounds.top().max(backdrop_bounds.top());
    let clip_bottom = input_bounds.bottom().min(backdrop_bounds.bottom());
    if clip_bottom <= clip_top || backdrop_bounds.size.width <= px(0.) {
        return;
    }
    let clip_bounds = Bounds::new(
        point(backdrop_bounds.left(), clip_top),
        size(backdrop_bounds.size.width, clip_bottom - clip_top),
    );
    let origin_y = input_bounds.origin.y + scroll_offset.y;
    let first_visible_row = (((clip_top - origin_y) / line_height).floor().max(0.)) as usize;
    let start_ix = entries.partition_point(|entry| entry.row < first_visible_row);

    window.with_content_mask(
        Some(ContentMask {
            bounds: clip_bounds,
        }),
        |window| {
            for entry in &entries[start_ix..] {
                if entry.row >= lines_len {
                    break;
                }
                let row_top = origin_y + line_height * entry.row as f32;
                if row_top >= clip_bottom {
                    break;
                }
                let row_bottom = row_top + line_height;
                if row_bottom <= clip_top {
                    continue;
                }
                let rect = Bounds::new(
                    point(backdrop_bounds.left(), row_top),
                    size(backdrop_bounds.size.width, line_height),
                );
                window.paint_quad(fill(rect, error_bg));
                #[cfg(test)]
                painted_out.borrow_mut().push((entry.row + 1, rect));
            }
        },
    );
}

struct ErrorHoverPopover {
    editor: Entity<EditorState>,
    source_editor: WeakEntity<SourceEditor>,
    row: usize,
    anchor_x: Pixels,
    messages: Vec<SharedString>,
}

struct ErrorHoverLayoutState {
    size: gpui_kit::Size<Pixels>,
    element: AnyElement,
}

impl IntoElement for ErrorHoverPopover {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ErrorHoverPopover {
    type PrepaintState = Option<Point<Pixels>>;
    type RequestLayoutState = ErrorHoverLayoutState;

    fn id(&self) -> Option<ElementId> {
        Some("source-editor-error-hover-anchor".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        const EDGE_MARGIN: Pixels = px(8.);
        let max_width = (window.bounds().size.width - EDGE_MARGIN * 2.)
            .min(px(560.))
            .max(px(200.));
        let theme = cx.theme();
        let hover_bg =
            theme
                .popover
                .blend(theme.red.opacity(if theme.is_dark() { 0.16 } else { 0.08 }));
        let hover_border = theme.red.opacity(if theme.is_dark() { 0.6 } else { 0.45 });
        let fg = theme.popover_foreground;
        let red = theme.red;
        let combined_label: SharedString = self
            .messages
            .iter()
            .map(SharedString::as_ref)
            .collect::<Vec<_>>()
            .join("\n")
            .into();

        let font_size = theme.mono_font_size;
        let rows = self.messages.iter().map(|msg| {
            h_flex()
                .items_start()
                .gap_1p5()
                .child(
                    Icon::new(AssetIconName::TriangleAlert)
                        .mt(px(3.))
                        .flex_shrink_0()
                        .text_color(red),
                )
                .child(div().flex_1().min_w_0().text_color(fg).child(msg.clone()))
        });

        let mut popover = deferred(
            div()
                .id("source-editor-error-hover")
                .test_support()
                .aria_label(combined_label)
                .flex_none()
                .v_flex()
                .gap_1()
                .px_2p5()
                .py_1p5()
                .text_size(font_size)
                .rounded(theme.radius)
                .bg(hover_bg)
                .border_1()
                .border_color(hover_border)
                .shadow_md()
                .max_w(max_width)
                .children(rows),
        )
        .into_any_element();

        let popover_size = popover.layout_as_root(AvailableSpace::min_size(), window, cx);
        let layout_id = div().into_any_element().request_layout(window, cx);
        (
            layout_id,
            ErrorHoverLayoutState {
                size: popover_size,
                element: popover,
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        const EDGE_MARGIN: Pixels = px(8.);
        const ROW_GAP: Pixels = px(4.);

        let state = self.editor.read(cx);
        let (input_bounds, line_height, scroll_offset) = editor_render_viewport_geometry(state)?;
        if self.row >= state.text().lines_len() {
            return None;
        }
        let origin_y = input_bounds.origin.y + scroll_offset.y;
        let line_top = origin_y + line_height * self.row as f32;
        let line_bottom = line_top + line_height;
        if line_bottom <= input_bounds.top() || line_top >= input_bounds.bottom() {
            return None;
        }

        let popover_size = request_layout.size;
        let y = if line_top - EDGE_MARGIN >= popover_size.height + ROW_GAP {
            line_top - popover_size.height - ROW_GAP
        } else {
            line_bottom + ROW_GAP
        };
        let min_x = (input_bounds.left() + px(36.)).max(EDGE_MARGIN);
        let max_x =
            (window.bounds().size.width - popover_size.width - EDGE_MARGIN).max(EDGE_MARGIN);
        let x = (self.anchor_x - px(12.)).max(min_x).min(max_x);
        let origin = point(x, y);

        window.with_absolute_element_offset(origin, |window| {
            request_layout.element.prepaint(window, cx);
        });
        Some(origin)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if prepaint.is_none() {
            return;
        }
        request_layout.element.paint(window, cx);

        let source_editor = self.source_editor.clone();
        window.on_mouse_event(move |_: &MouseDownEvent, _, _, cx| {
            let _ = source_editor.update(cx, |this, cx| this.clear_hovered_error(cx));
        });
        let source_editor = self.source_editor.clone();
        window.on_mouse_event(move |_: &ScrollEvent, _, _, cx| {
            let _ = source_editor.update(cx, |this, cx| this.clear_hovered_error(cx));
        });
    }
}

#[cfg(test)]
#[path = "source_editor_test.rs"]
mod tests;

fn scroll_line_to_top(state: &mut EditorState, row: u32, cx: &mut Context<EditorState>) -> bool {
    let Some(line_height) = state.line_height() else {
        return false;
    };
    let Some(bounds) = state.text_bounds().filter(|b| b.size.height > px(0.)) else {
        return false;
    };
    let desired_y = -(row as f32 * line_height);
    let content_height = state.text().lines_len() as f32 * line_height;
    let min_y = (bounds.size.height - content_height).min(px(0.));
    state.set_scroll_offset(point(px(0.), desired_y.max(min_y)), cx);
    true
}

fn schedule_scroll_to_top(
    weak: WeakEntity<EditorState>,
    applied: Position,
    attempts_left: u8,
    window: &mut Window,
) {
    if attempts_left == 0 {
        return;
    }
    window.on_next_frame(move |window, cx| {
        let Some(text) = weak.upgrade() else {
            return;
        };
        let done = text.update(cx, |state, cx| {
            if state.cursor_position() != applied {
                return true;
            }
            scroll_line_to_top(state, applied.line, cx)
        });
        if !done {
            schedule_scroll_to_top(weak, applied, attempts_left - 1, window);
        }
    });
}

fn move_cursor_to_line(text: &Entity<EditorState>, line: usize, window: &mut Window, cx: &mut App) {
    let target_line = line.saturating_sub(1) as u32;
    let applied = text.update(cx, |state, cx| {
        let last_line = state.text().lines_len().saturating_sub(1) as u32;
        let pos = Position::new(target_line.min(last_line), 0);
        state.set_cursor_position(pos, window, cx);
        scroll_line_to_top(state, pos.line, cx);
        pos
    });
    // `line_height()` and `text_bounds()` require a completed layout pass when
    // `Page::Editor` is painted for the first time; re-applying on the next
    // frame guarantees `applied` lands at the top of the viewport (clamped at
    // the end of the file).
    schedule_scroll_to_top(text.downgrade(), applied, 3, window);
}

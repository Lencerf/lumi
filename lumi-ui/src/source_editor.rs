use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, InputEvent, Search};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, StyledExt as _, h_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window, actions,
    div, px,
};

use crate::completion::BeancountCompletionProvider;
use crate::errors_report::common_dir;
use crate::session::LedgerSession;
use crate::style::table_card;
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

struct Draft {
    original: SharedString,
    text: SharedString,
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
    provider: Rc<BeancountCompletionProvider>,
    files: Vec<SharedString>,
    selected_file: Option<SharedString>,
    /// Text of `selected_file` as last read from or written to disk.
    original: Option<SharedString>,
    /// Unsaved buffers for non-active files when switching files in the dropdown.
    drafts: HashMap<SharedString, Draft>,
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
        let text = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language(highlight::LANGUAGE)
                .searchable(true);
            state.set_soft_wrap(false, window, cx);
            state.set_line_number(true, window, cx);
            state.set_folding(false, window, cx);
            state.set_highlighter_factory(highlight::highlighter_factory(), cx);
            let lsp = state.lsp_mut();
            lsp.completion_provider = Some(provider_for_state);
            lsp.completion_menu.max_width = px(480.);
            lsp.set_completion_highlighter(completion::highlighter());
            state
        });
        let file_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let _subscriptions = vec![
            cx.observe_in(&session, window, |this, _, window, cx| {
                this.on_session_changed(window, cx)
            }),
            cx.subscribe_in(&file_select, window, Self::on_select_event),
            cx.subscribe(&text, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            session,
            file_select,
            text,
            provider,
            files: Vec::new(),
            selected_file: None,
            original: None,
            drafts: HashMap::new(),
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

    #[cfg(test)]
    pub(crate) fn editor_state(&self) -> &Entity<EditorState> {
        &self.text
    }

    /// Focuses the code editor and opens its in-file search bar.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.text.update(cx, |state, cx| {
            state.focus(window, cx);
            state.open_search(false, cx);
        });
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
            if focus_editor {
                self.text.update(cx, |state, cx| state.focus(window, cx));
            }
            return;
        }

        // Preserve any unsaved edits on the file we are leaving.
        if let (Some(prev_path), Some(orig)) = (&self.selected_file, &self.original) {
            let current = self.text.read(cx).value();
            if current != *orig {
                self.drafts.insert(
                    prev_path.clone(),
                    Draft {
                        original: orig.clone(),
                        text: current,
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
            self.original = Some(draft.original);
            self.text.update(cx, |state, cx| {
                state.set_value(draft.text, window, cx);
                if focus_editor {
                    state.focus(window, cx);
                }
            });
            cx.notify();
            return;
        }

        self.original = None;
        self.load_file_text(path, focus_editor, window, cx);
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
        self._meta_task = Some(cx.spawn_in(window, async move |this, cx| {
            let (files_res, completions_res) = (files_task.await, completions_task.await);
            this.update_in(cx, |this, window, cx| {
                if this.meta_generation != generation {
                    return;
                }
                if let Ok(completions) = completions_res {
                    this.provider.set_completions(completions);
                }
                match files_res {
                    Ok(raw_files) => {
                        let items = build_file_items(&raw_files);
                        this.files = items.iter().map(|item| item.path.clone()).collect();
                        let target = this
                            .selected_file
                            .as_ref()
                            .filter(|selected| this.files.contains(selected))
                            .cloned()
                            .or_else(|| this.files.first().cloned());
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
                                }
                                this.load_file_text(target, false, window, cx);
                            }
                        } else {
                            this.selected_file = None;
                            this.original = None;
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
                            this.text.update(cx, |state, cx| {
                                state.set_value(content, window, cx);
                                if focus_editor {
                                    state.focus(window, cx);
                                }
                            });
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
        self.text.update(cx, |state, cx| {
            state.set_value(original, window, cx);
            state.focus(window, cx);
        });
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
                            this.original = Some(new_text);
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

        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(|this, _: &Search, window, cx| this.focus_search(window, cx)))
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
                        .relative()
                        .size_full()
                        .child(
                            Editor::new(&self.text)
                                .size_full()
                                .bordered(false)
                                .aria_label("Source text")
                                .disabled(!loaded || saving),
                        )
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
                        }),
                ),
            )
    }
}

#[cfg(test)]
#[path = "source_editor_test.rs"]
mod tests;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Editor, EditorState, Enter, Escape, InputEvent};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Empty, Entity, EntityId, EventEmitter, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TestSupportExt as _,
    WeakEntity, WeakFocusHandle, Window, actions, div, px,
};
use lumi::{Source, Transaction};

use crate::align::PostingAligner;
use crate::completion::BeancountCompletionProvider;
use crate::input_guard::EditDragGuard;
use crate::session::LedgerSession;
use crate::style::interactive_button;
use crate::{completion, highlight};

actions!(lumi_ui, [CancelEntry, SaveEntry]);

/// Request from [`EntryEditor`] to open `file` at 1-based `line` in [`crate::SourceEditor`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OpenSource {
    pub(crate) file: SharedString,
    pub(crate) line: usize,
}

/// The key context of the entry editor; its key bindings apply within it.
pub(crate) const KEY_CONTEXT: &str = "EntryEditor";

/// Edits, deletes, or duplicates one entry of the ledger, in a dialog.
///
/// The text comes from the ledger file through the session's source and
/// goes back the same way; the ledger is always reloaded after a write, so
/// every report shows the change. Replacing or deleting an entry is refused
/// when the entry changed on disk in the meantime, and the editor stays open
/// with the reason.
pub struct EntryEditor {
    session: Entity<LedgerSession>,
    source: Source,
    text: Entity<EditorState>,
    drag_guard: EditDragGuard,
    aligner: PostingAligner,
    provider: Rc<BeancountCompletionProvider>,
    /// The entry's text as read; `None` until it has arrived.
    original: Option<SharedString>,
    /// Whether any edit has occurred since the dialog opened, even if later
    /// undone or edited back to match `original`.
    edited: bool,
    /// When `true`, saving appends the text as a new entry in `source.file`
    /// rather than replacing the entry that was read.
    duplicating: bool,
    /// Recorded before action dispatch on each keystroke so `Escape` dismisses
    /// only an open completion menu instead of also cancelling the dialog.
    dismissing_overlay: Rc<Cell<bool>>,
    pending: Option<PendingWrite>,
    error: Option<SharedString>,
    _task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingWrite {
    Saving,
    Deleting,
}

#[derive(Default)]
struct EntryEditorRegistry {
    layers: HashMap<AnyWindowHandle, WeakEntity<EntryEditorLayer>>,
}

impl Global for EntryEditorRegistry {}

struct ActiveEditor {
    editor: Entity<EntryEditor>,
    previous_focus: Option<WeakFocusHandle>,
    _subscriptions: [Subscription; 2],
}

/// Modal layer mounted inside [`crate::LumiApp`] in the base draw pass so that
/// [`Editor`]'s completion popover (which GPUI Kit 0.8.1 defers at priority `0`)
/// paints above the dialog rather than behind `gpui_base::Dialog` (priority `10`).
pub(crate) struct EntryEditorLayer {
    active: Option<ActiveEditor>,
}

impl EventEmitter<OpenSource> for EntryEditorLayer {}

impl EntryEditorLayer {
    pub(crate) fn is_open(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn register(window: &Window, cx: &mut App) -> Entity<Self> {
        let layer = cx.new(|_| Self { active: None });
        if !cx.has_global::<EntryEditorRegistry>() {
            cx.set_global(EntryEditorRegistry::default());
        }
        cx.global_mut::<EntryEditorRegistry>()
            .layers
            .insert(window.window_handle(), layer.downgrade());
        layer
    }

    fn show(editor: Entity<EntryEditor>, window: &mut Window, cx: &mut App) {
        let Some(layer) = cx
            .try_global::<EntryEditorRegistry>()
            .and_then(|reg| reg.layers.get(&window.window_handle()))
            .and_then(WeakEntity::upgrade)
        else {
            return;
        };
        let previous_focus = window.focused(cx).map(|h| h.downgrade());
        layer.update(cx, |this, cx| {
            let previous_focus = this
                .active
                .take()
                .and_then(|prev| prev.previous_focus)
                .or(previous_focus);
            let subscriptions = [
                cx.observe(&editor, |_, _, cx| cx.notify()),
                cx.subscribe(&editor, |this, _, event: &OpenSource, cx| {
                    let event = event.clone();
                    this.active = None;
                    cx.notify();
                    cx.emit(event);
                }),
            ];
            this.active = Some(ActiveEditor {
                editor,
                previous_focus,
                _subscriptions: subscriptions,
            });
            cx.notify();
        });
    }

    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(active) = self.active.take() {
            if let Some(focus) = active.previous_focus.and_then(|h| h.upgrade()) {
                window.focus(&focus, cx);
            }
            cx.notify();
        }
    }

    fn close_editor(editor_id: EntityId, window: &mut Window, cx: &mut App) {
        let Some(layer) = cx
            .try_global::<EntryEditorRegistry>()
            .and_then(|reg| reg.layers.get(&window.window_handle()))
            .and_then(WeakEntity::upgrade)
        else {
            return;
        };
        layer.update(cx, |this, cx| {
            if this
                .active
                .as_ref()
                .is_some_and(|active| active.editor.entity_id() == editor_id)
            {
                this.close(window, cx);
            }
        });
    }
}

impl Render for EntryEditorLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(active) = self.active.as_ref() else {
            return Empty.into_any_element();
        };
        let editor = active.editor.clone();
        let (duplicating, busy) = {
            let state = editor.read(cx);
            (
                state.is_duplicating(),
                state.is_saving() || state.is_deleting(),
            )
        };
        let title = if duplicating {
            "Duplicate entry"
        } else {
            "Edit entry"
        };
        let viewport = window.viewport_size();
        div()
            .id("entry-editor-backdrop")
            .test_support()
            .absolute()
            .inset_0()
            .occlude()
            .bg(cx.theme().overlay)
            .flex()
            .items_start()
            .justify_center()
            .pt(viewport.height / 10.)
            .px_4()
            .on_click({
                let editor = editor.clone();
                move |_, window, cx| {
                    editor.update(cx, |this, cx| {
                        if !this.has_edited(cx) {
                            this.close(window, cx);
                        }
                    });
                }
            })
            .child(
                div()
                    .id("entry-editor-dialog")
                    .test_support()
                    .occlude()
                    .v_flex()
                    .w(px(720.))
                    .max_w_full()
                    .max_h(viewport.height - px(32.))
                    .p_4()
                    .gap_3()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_lg)
                    .shadow_xl()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_lg().font_semibold().child(title))
                            .child(
                                Button::new("close-entry-editor")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .disabled(busy)
                                    .on_click({
                                        let editor = editor.clone();
                                        move |_, window, cx| {
                                            editor.update(cx, |this, cx| this.close(window, cx));
                                        }
                                    }),
                            ),
                    )
                    .child(editor),
            )
            .into_any_element()
    }
}

impl EntryEditor {
    /// Opens a dialog editing `txn` where it is written in the ledger, and
    /// returns the editor behind it.
    pub fn open(
        txn: &Transaction,
        session: Entity<LedgerSession>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let editor = cx.new(|cx| Self::new(txn.src().clone(), session, window, cx));
        EntryEditorLayer::show(editor.clone(), window, cx);
        editor
    }

    fn new(
        source: Source,
        session: Entity<LedgerSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let provider = BeancountCompletionProvider::new();
        let provider_for_state = provider.clone();
        let text = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language(highlight::LANGUAGE)
                .searchable(false);
            state.set_soft_wrap(false, window, cx);
            state.set_line_number(false, window, cx);
            state.set_folding(false, window, cx);
            state.set_highlighter_factory(highlight::highlighter_factory(), cx);
            let lsp = state.lsp_mut();
            lsp.completion_provider = Some(provider_for_state);
            lsp.completion_menu.max_width = px(480.);
            lsp.set_completion_highlighter(completion::highlighter());
            state
        });
        let drag_guard = EditDragGuard::new(&text);
        let aligner = PostingAligner::new(&text, false, cx);
        let dismissing_overlay = Rc::new(Cell::new(false));
        let _subscriptions = vec![
            cx.subscribe_in(&text, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.edited = true;
                    this.drag_guard.cancel();
                    this.aligner
                        .on_change(&this.text, &this.provider, window, cx);
                    cx.notify();
                }
            }),
            cx.intercept_keystrokes({
                let text = text.downgrade();
                let dismissing_overlay = dismissing_overlay.clone();
                move |_, _, cx| {
                    let open = text
                        .read_with(cx, |state, _| state.completion_menu_state().open)
                        .unwrap_or(false);
                    dismissing_overlay.set(open);
                }
            }),
        ];
        let ledger = session.read(cx).source().clone();
        let entry_task = ledger.entry_text(&source, cx);
        let completions_task = ledger.completions(cx);
        let _task = Some(cx.spawn_in(window, async move |this, cx| {
            let (result, completions) = (entry_task.await, completions_task.await);
            this.update_in(cx, |this, window, cx| {
                if let Ok(completions) = completions {
                    this.provider.set_completions(completions);
                }
                match result {
                    Ok(text) => {
                        this.text.update(cx, |state, cx| {
                            state.set_value(text.clone(), window, cx);
                            state.focus(window, cx);
                        });
                        this.original = Some(text.into());
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
        Self {
            session,
            source,
            text,
            drag_guard,
            aligner,
            provider,
            original: None,
            edited: false,
            duplicating: false,
            dismissing_overlay,
            pending: None,
            error: None,
            _task,
            _subscriptions,
        }
    }

    /// Where the entry is (`file:line`), or where a duplicate will be added.
    pub fn location(&self) -> String {
        if self.duplicating {
            format!("New entry in {}", self.source.file)
        } else {
            format!("{}:{}", self.source.file, self.source.start.line)
        }
    }

    /// The text as read from the ledger, once it has arrived.
    pub fn original(&self) -> Option<&SharedString> {
        self.original.as_ref()
    }

    /// Whether the editor is preparing a duplicated copy to append.
    pub fn is_duplicating(&self) -> bool {
        self.duplicating
    }

    /// Whether the text differs from what was read.
    pub fn is_changed(&self, cx: &App) -> bool {
        self.original
            .as_ref()
            .is_some_and(|original| *original != self.text.read(cx).value())
    }

    /// Whether any edit has occurred since the dialog opened, including edits
    /// later reverted back to the original text or switching to duplicate mode.
    pub fn has_edited(&self, cx: &App) -> bool {
        self.edited || self.duplicating || self.is_changed(cx)
    }

    pub fn is_saving(&self) -> bool {
        self.pending == Some(PendingWrite::Saving)
    }

    pub fn is_deleting(&self) -> bool {
        self.pending == Some(PendingWrite::Deleting)
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn editor_state(&self) -> &Entity<EditorState> {
        &self.text
    }

    fn can_save(&self, cx: &App) -> bool {
        if self.duplicating {
            !self.text.read(cx).value().trim().is_empty()
        } else {
            self.is_changed(cx)
        }
    }

    /// Turns the editor into a new-entry copy dated today, ready to save or
    /// adjust before saving.
    pub fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.original.is_none() || self.duplicating || self.pending.is_some() {
            return;
        }
        let updated = lumi::edit::with_today(&self.text.read(cx).value());
        self.text.update(cx, |state, cx| {
            state.set_value(updated, window, cx);
            state.focus(window, cx);
        });
        self.edited = true;
        self.duplicating = true;
        self.error = None;
        window.refresh();
        cx.notify();
    }

    /// Writes the text to the ledger — replacing the entry or appending its
    /// duplicate — then reloads the ledger and closes the dialog.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() || !self.can_save(cx) {
            return;
        }
        let text = self.text.read(cx).value().to_string();
        let ledger = self.session.read(cx).source().clone();
        let task = if self.duplicating {
            ledger.append_entry_text(&self.source.file, text, cx)
        } else {
            let Some(original) = self.original.clone() else {
                return;
            };
            ledger.replace_entry_text(&self.source, original.to_string(), text, cx)
        };
        self.run_write(PendingWrite::Saving, task, window, cx);
    }

    /// Deletes the entry from its ledger file, reloads the ledger and closes
    /// the dialog.
    pub fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.duplicating || self.pending.is_some() {
            return;
        }
        let Some(original) = self.original.clone() else {
            return;
        };
        let ledger = self.session.read(cx).source().clone();
        let task = ledger.delete_entry_text(&self.source, original.to_string(), cx);
        self.run_write(PendingWrite::Deleting, task, window, cx);
    }

    fn run_write(
        &mut self,
        kind: PendingWrite,
        task: Task<anyhow::Result<()>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = Some(kind);
        self.error = None;
        cx.notify();
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.pending = None;
                match result {
                    Ok(()) => {
                        this.session.update(cx, |session, cx| session.reload(cx));
                        this.close(window, cx);
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Closes the editor modal if no write is in flight.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        EntryEditorLayer::close_editor(cx.entity().entity_id(), window, cx);
    }

    /// Closes the dialog and asks the application to open `source.file` in the
    /// built-in editor with the cursor at `source.start.line`.
    pub fn open_in_editor(&mut self, cx: &mut Context<Self>) {
        if self.duplicating || self.pending.is_some() {
            return;
        }
        cx.emit(OpenSource {
            file: self.source.file.as_str().into(),
            line: self.source.start.line,
        });
    }

    fn on_save(&mut self, _: &SaveEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.save(window, cx);
    }

    fn on_cancel(&mut self, _: &CancelEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
    }

    fn on_editor_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dismissing_overlay.replace(false) {
            self.close(window, cx);
        }
    }

    fn on_editor_enter(&mut self, _: &Enter, _: &mut Window, _: &mut Context<Self>) {}
}

impl EventEmitter<OpenSource> for EntryEditor {}

impl Render for EntryEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let loaded = self.original.is_some();
        let busy = self.pending.is_some();
        let can_save = self.can_save(cx);
        let saving = self.is_saving();
        let deleting = self.is_deleting();
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_cancel))
            .on_action(cx.listener(Self::on_editor_escape))
            .on_action(cx.listener(Self::on_editor_enter))
            .v_flex()
            .gap_3()
            .child(if self.duplicating {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(self.location())
                    .into_any_element()
            } else {
                let location = self.location();
                h_flex()
                    .child(
                        interactive_button("open-in-editor", cx)
                            .font_family(cx.theme().mono_font_family.clone())
                            .label(location.clone())
                            .accessibility_label(format!("Open {location} in editor"))
                            .tooltip("Open in editor")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.open_in_editor(cx))),
                    )
                    .into_any_element()
            })
            .child(
                div()
                    .relative()
                    .capture_action(self.drag_guard.paste_capture())
                    .capture_action(self.aligner.undo_capture())
                    .capture_action(self.aligner.redo_capture())
                    .child(self.drag_guard.element())
                    .child(
                        Editor::new(&self.text)
                            .h(px(320.))
                            .aria_label("Entry text")
                            .disabled(!loaded || busy),
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
            )
            .when_some(self.error.clone(), |this, message| {
                this.child(Alert::error("entry-editor-error", message))
            })
            .child(
                // Left and right groups keep each button to its own width.
                DialogFooter::new()
                    .justify_between()
                    .child(h_flex().gap_2().when(!self.duplicating, |this| {
                        this.child(
                            Button::new("duplicate-entry")
                                .outline()
                                .label("Duplicate")
                                .disabled(!loaded || busy)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.duplicate(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("delete-entry")
                                .danger()
                                .outline()
                                .label("Delete")
                                .disabled(!loaded || busy)
                                .loading(deleting)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.delete(window, cx)),
                                ),
                        )
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("cancel")
                                    .label("Cancel")
                                    .outline()
                                    .disabled(busy)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.close(window, cx)),
                                    ),
                            )
                            .child(
                                Button::new("save-entry")
                                    .primary()
                                    .label("Save")
                                    .disabled(!can_save || busy)
                                    .loading(saving)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.save(window, cx)),
                                    ),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
#[path = "entry_editor_test.rs"]
mod tests;

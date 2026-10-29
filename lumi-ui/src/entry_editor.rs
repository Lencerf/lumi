use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogClose, DialogFooter};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, StyledExt as _, WindowExt as _, h_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window, actions,
    div, px,
};
use lumi::{Source, Transaction};

use crate::session::LedgerSession;

actions!(lumi_ui, [SaveEntry]);

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
    text: Entity<TextareaState>,
    /// The entry's text as read; `None` until it has arrived.
    original: Option<SharedString>,
    /// When `true`, saving appends the text as a new entry in `source.file`
    /// rather than replacing the entry that was read.
    duplicating: bool,
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
        window.open_dialog(cx, {
            let editor = editor.clone();
            move |dialog, _, cx| {
                let title = if editor.read(cx).is_duplicating() {
                    "Duplicate entry"
                } else {
                    "Edit entry"
                };
                dialog
                    .title(title)
                    .w(px(720.))
                    // Edits are not lost to a click beside the dialog; Cancel,
                    // the close button and Escape leave it.
                    .overlay_closable(false)
                    .child(editor.clone())
            }
        });
        editor
    }

    fn new(
        source: Source,
        session: Entity<LedgerSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = cx.new(|cx| TextareaState::new(window, cx).soft_wrap(false));
        let _subscriptions = vec![cx.subscribe(&text, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        let ledger = session.read(cx).source().clone();
        let task = ledger.entry_text(&source, cx);
        let _task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
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
            original: None,
            duplicating: false,
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

    pub fn is_saving(&self) -> bool {
        self.pending == Some(PendingWrite::Saving)
    }

    pub fn is_deleting(&self) -> bool {
        self.pending == Some(PendingWrite::Deleting)
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
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
                        window.close_dialog(cx);
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn on_save(&mut self, _: &SaveEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.save(window, cx);
    }
}

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
            .v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(self.location()),
            )
            .child(
                div()
                    .relative()
                    .child(
                        Textarea::new(&self.text)
                            .h(px(320.))
                            .font_family(cx.theme().mono_font_family.clone())
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
                                DialogClose::new().child(
                                    Button::new("cancel")
                                        .label("Cancel")
                                        .outline()
                                        .disabled(busy),
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

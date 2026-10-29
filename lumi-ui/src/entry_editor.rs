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

/// Edits the text of one entry of the ledger, in a dialog.
///
/// The text comes from the ledger file through the session's source and
/// goes back the same way; the ledger is reloaded after a save, so every
/// report shows the change. Saving is refused when the entry changed on
/// disk in the meantime, and the editor stays open with the reason.
pub struct EntryEditor {
    session: Entity<LedgerSession>,
    source: Source,
    text: Entity<TextareaState>,
    /// The entry's text as read; `None` until it has arrived.
    original: Option<SharedString>,
    saving: bool,
    error: Option<SharedString>,
    _task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
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
            move |dialog, _, _| {
                dialog
                    .title("Edit entry")
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
            saving: false,
            error: None,
            _task,
            _subscriptions,
        }
    }

    /// Where the entry is: `file:line`.
    pub fn location(&self) -> String {
        format!("{}:{}", self.source.file, self.source.start.line)
    }

    /// The text as read from the ledger, once it has arrived.
    pub fn original(&self) -> Option<&SharedString> {
        self.original.as_ref()
    }

    /// Whether the text differs from what was read.
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

    /// Writes the text to the ledger, then reloads the ledger and closes
    /// the dialog. Nothing happens while the text is unchanged or a save
    /// is in flight.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(original) = self.original.clone() else {
            return;
        };
        let text = self.text.read(cx).value();
        if self.saving || text == original {
            return;
        }
        self.saving = true;
        self.error = None;
        cx.notify();
        let ledger = self.session.read(cx).source().clone();
        let task =
            ledger.replace_entry_text(&self.source, original.to_string(), text.to_string(), cx);
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.saving = false;
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
        let changed = self.is_changed(cx);
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
                            .disabled(!loaded),
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
                // Grouped, so that neither button is stretched across the
                // footer.
                DialogFooter::new().child(
                    h_flex()
                        .gap_2()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel").label("Cancel").outline()),
                        )
                        .child(
                            Button::new("save-entry")
                                .primary()
                                .label("Save")
                                .disabled(!changed || self.saving)
                                .loading(self.saving)
                                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                        ),
                ),
            )
    }
}

#[cfg(test)]
#[path = "entry_editor_test.rs"]
mod tests;

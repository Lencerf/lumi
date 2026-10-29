use std::sync::Arc;

use gpui_kit::{Context, SharedString, Task};

use crate::source::LedgerSource;

/// The ledger the application is showing, and whether it is being reloaded.
///
/// Views hold an `Entity<LedgerSession>` and observe it: `revision` changes
/// after every successful reload, which is their cue to query the
/// [`LedgerSource`] again. A session may start empty (before the user has
/// chosen a ledger file) and switch sources later with [`set_source`](Self::set_source).
/// The session owns the reload task, so dropping or switching the session
/// cancels a reload in flight.
pub struct LedgerSession {
    source: Option<Arc<dyn LedgerSource>>,
    revision: u64,
    loaded: bool,
    reloading: bool,
    last_error: Option<SharedString>,
    _reload_task: Option<Task<()>>,
}

impl LedgerSession {
    /// A session with no ledger chosen yet.
    pub fn empty() -> Self {
        Self {
            source: None,
            revision: 0,
            loaded: false,
            reloading: false,
            last_error: None,
            _reload_task: None,
        }
    }

    /// A session over `source`. Nothing is loaded until [`reload`](Self::reload).
    pub fn new(source: Arc<dyn LedgerSource>) -> Self {
        Self {
            source: Some(source),
            ..Self::empty()
        }
    }

    pub fn has_source(&self) -> bool {
        self.source.is_some()
    }

    pub fn try_source(&self) -> Option<&Arc<dyn LedgerSource>> {
        self.source.as_ref()
    }

    /// The current ledger source. Panics if no ledger has been set yet; check
    /// [`has_source`](Self::has_source) when a session may be empty.
    pub fn source(&self) -> &Arc<dyn LedgerSource> {
        self.source
            .as_ref()
            .expect("the session has no ledger source")
    }

    /// Increases after every successful reload; `0` until the first one.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the current ledger source has completed at least one reload.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn is_reloading(&self) -> bool {
        self.reloading
    }

    /// Why the last reload failed, until a reload succeeds.
    pub fn last_error(&self) -> Option<&SharedString> {
        self.last_error.as_ref()
    }

    /// Replaces the ledger source and starts loading it, cancelling any reload
    /// of the previous source.
    pub fn set_source(&mut self, source: Arc<dyn LedgerSource>, cx: &mut Context<Self>) {
        self.source = Some(source);
        self.loaded = false;
        self.reloading = false;
        self.last_error = None;
        self._reload_task = None;
        self.reload(cx);
    }

    /// Reads the ledger again. Nothing happens if no source is set or a reload
    /// is already in flight.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.source.clone() else {
            return;
        };
        if self.reloading {
            return;
        }
        self.reloading = true;
        cx.notify();
        let task = source.reload(cx);
        self._reload_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.reloading = false;
                match result {
                    Ok(()) => {
                        this.revision += 1;
                        this.loaded = true;
                        this.last_error = None;
                    }
                    Err(error) => this.last_error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }
}

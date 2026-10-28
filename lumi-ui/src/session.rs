use std::sync::Arc;

use gpui_kit::{Context, SharedString, Task};

use crate::source::LedgerSource;

/// The ledger the application is showing, and whether it is being reloaded.
///
/// Views hold an `Entity<LedgerSession>` and observe it: `revision` changes
/// after every successful reload, which is their cue to query the
/// [`LedgerSource`] again. The session owns the reload task, so dropping the
/// session cancels a reload in flight.
pub struct LedgerSession {
    source: Arc<dyn LedgerSource>,
    revision: u64,
    reloading: bool,
    last_error: Option<SharedString>,
    _reload_task: Option<Task<()>>,
}

impl LedgerSession {
    /// A session over `source`. Nothing is loaded until [`reload`](Self::reload).
    pub fn new(source: Arc<dyn LedgerSource>) -> Self {
        Self {
            source,
            revision: 0,
            reloading: false,
            last_error: None,
            _reload_task: None,
        }
    }

    pub fn source(&self) -> &Arc<dyn LedgerSource> {
        &self.source
    }

    /// Increases after every successful reload; `0` until the first one.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_loaded(&self) -> bool {
        self.revision > 0
    }

    pub fn is_reloading(&self) -> bool {
        self.reloading
    }

    /// Why the last reload failed, until a reload succeeds.
    pub fn last_error(&self) -> Option<&SharedString> {
        self.last_error.as_ref()
    }

    /// Reads the ledger again. A reload already in flight is left alone.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.reloading {
            return;
        }
        self.reloading = true;
        cx.notify();
        let task = self.source.reload(cx);
        self._reload_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.reloading = false;
                match result {
                    Ok(()) => {
                        this.revision += 1;
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

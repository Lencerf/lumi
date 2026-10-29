use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::sidebar::{
    Sidebar, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Render,
    SharedString, Styled as _, Subscription, Task, Window, actions, div,
};

use crate::errors_report::ErrorsReport;
use crate::holdings_report::HoldingsReport;
use crate::journal_report::JournalReport;
use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::source::LedgerSource;
use crate::trie_report::TrieReport;

actions!(lumi_ui, [GoBack, OpenLedger, Refresh]);

/// The key context of the application view; its key bindings apply within it.
pub(crate) const KEY_CONTEXT: &str = "LumiApp";

/// Builds a [`LedgerSource`] for a ledger file path chosen in the interface.
pub type PathOpener = Arc<dyn Fn(&Path) -> Arc<dyn LedgerSource>>;

#[cfg(not(target_family = "wasm"))]
fn default_path_opener() -> Option<PathOpener> {
    Some(Arc::new(|path: &Path| {
        Arc::new(crate::source::LocalLedger::new(
            path.to_string_lossy().into_owned(),
        ))
    }))
}

#[cfg(target_family = "wasm")]
fn default_path_opener() -> Option<PathOpener> {
    None
}

/// The short display name of a ledger file at `path`.
pub fn ledger_name_for_path(path: &Path) -> SharedString {
    path.file_name()
        .map_or_else(
            || path.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
        .into()
}

/// The application window: a sidebar of reports beside the selected report,
/// with the current root file in the upper-left corner and ledger commands in
/// the header.
pub struct LumiApp {
    session: Entity<LedgerSession>,
    ledger_name: Option<SharedString>,
    ledger_path: Option<SharedString>,
    path_opener: Option<PathOpener>,
    open_error: Option<SharedString>,
    page: Page,
    /// Pages left behind when opening an account view, so "Go back" returns to
    /// the previous view.
    back_stack: Vec<Page>,
    /// Reports are created the first time they are opened and kept, so that
    /// returning to one does not query the ledger again; cleared when
    /// switching to another root file.
    reports: HashMap<Page, Report>,
    focus_handle: FocusHandle,
    _open_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// The view behind a page, and the subscription that routes its requests to
/// the application, for the views that make any.
struct Report {
    view: AnyView,
    _subscription: Option<Subscription>,
}

impl LumiApp {
    /// Opens `source`, named `ledger_name` in the sidebar, showing `page`, and
    /// starts loading it.
    pub fn new(
        source: Arc<dyn LedgerSource>,
        ledger_name: impl Into<SharedString>,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::empty(page, window, cx);
        this.set_ledger(source, ledger_name.into(), None, window, cx);
        this
    }

    /// Opens without a root ledger selected yet, ready for the user to choose
    /// one from the sidebar header or empty state.
    pub fn empty(page: Page, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session = cx.new(|_| LedgerSession::empty());
        let _subscriptions = vec![cx.observe(&session, |_, _, cx| cx.notify())];
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        Self {
            session,
            ledger_name: None,
            ledger_path: None,
            path_opener: default_path_opener(),
            open_error: None,
            page,
            back_stack: Vec::new(),
            reports: HashMap::new(),
            focus_handle,
            _open_task: None,
            _subscriptions,
        }
    }

    /// Opens a native window over `path`, or with no ledger yet when `path` is
    /// `None`.
    #[cfg(not(target_family = "wasm"))]
    pub fn new_local(
        path: Option<&Path>,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::empty(page, window, cx);
        match path {
            Some(path) => this.open_path(path, window, cx),
            None => window.set_window_title("Lumi"),
        }
        this
    }

    /// Supplies how a chosen file path becomes a [`LedgerSource`].
    pub fn with_path_opener(mut self, opener: PathOpener) -> Self {
        self.path_opener = Some(opener);
        self
    }

    pub fn page(&self) -> &Page {
        &self.page
    }

    pub fn ledger_name(&self) -> Option<&SharedString> {
        self.ledger_name.as_ref()
    }

    pub fn session(&self) -> &Entity<LedgerSession> {
        &self.session
    }

    /// The journal shown, if the page is one.
    pub fn journal(&self) -> Option<Entity<JournalReport>> {
        self.reports
            .get(&self.page)?
            .view
            .clone()
            .downcast::<JournalReport>()
            .ok()
    }

    /// Switches the application to `source`, named `ledger_name`, clearing
    /// cached reports from the previous ledger and loading the new one.
    pub fn open_ledger(
        &mut self,
        source: Arc<dyn LedgerSource>,
        ledger_name: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_ledger(source, ledger_name.into(), None, window, cx);
    }

    /// Switches the application to the ledger file at `path`.
    pub fn open_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(opener) = self.path_opener.clone() else {
            return;
        };
        let source = opener(path);
        let name = ledger_name_for_path(path);
        let full_path: SharedString = path.to_string_lossy().into_owned().into();
        self.set_ledger(source, name, Some(full_path), window, cx);
    }

    fn set_ledger(
        &mut self,
        source: Arc<dyn LedgerSource>,
        ledger_name: SharedString,
        ledger_path: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let had_source = self.session.read(cx).has_source();
        window.set_window_title(&format!("{ledger_name} — Lumi"));
        self.ledger_name = Some(ledger_name);
        self.ledger_path = ledger_path;
        self.open_error = None;
        self.back_stack.clear();
        self.reports.clear();
        // When switching ledgers, an account page belonged to the previous
        // ledger's chart of accounts.
        if had_source && self.page.account().is_some() {
            self.page = Page::BalanceSheet;
        }
        self.session
            .update(cx, |session, cx| session.set_source(source, cx));
        let page = self.page.clone();
        self.report(&page, window, cx);
        cx.notify();
    }

    /// Opens the platform file picker to choose a root ledger file.
    pub fn prompt_open_ledger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_opener.is_none() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select root ledger".into()),
        });
        self._open_task = Some(cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = receiver.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| match result {
                Ok(Some(paths)) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.open_path(&path, window, cx);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    this.open_error = Some(format!("{error:#}").into());
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn on_open_ledger(&mut self, _: &OpenLedger, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_open_ledger(window, cx);
    }

    /// The view for `page`, created on first use.
    fn report(&mut self, page: &Page, window: &mut Window, cx: &mut Context<Self>) -> AnyView {
        if let Some(report) = self.reports.get(page) {
            return report.view.clone();
        }
        let session = self.session.clone();
        let report = match page {
            Page::BalanceSheet | Page::Income => {
                let view = cx.new(|cx| TrieReport::new(page.clone(), session, cx));
                Report {
                    _subscription: Some(cx.subscribe(&view, Self::on_report_event)),
                    view: view.into(),
                }
            }
            Page::Journal | Page::Account(_) => {
                let view = cx.new(|cx| JournalReport::new(page.clone(), session, window, cx));
                Report {
                    _subscription: Some(cx.subscribe(&view, Self::on_report_event)),
                    view: view.into(),
                }
            }
            Page::Holdings => {
                let view = cx.new(|cx| HoldingsReport::new(session, window, cx));
                Report {
                    _subscription: Some(cx.subscribe(&view, Self::on_report_event)),
                    view: view.into(),
                }
            }
            Page::Errors => {
                let view = cx.new(|cx| ErrorsReport::new(session, window, cx));
                Report {
                    _subscription: None,
                    view: view.into(),
                }
            }
        };
        let view = report.view.clone();
        self.reports.insert(page.clone(), report);
        view
    }

    fn on_report_event<T: 'static>(
        &mut self,
        _: Entity<T>,
        event: &ReportEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            ReportEvent::OpenAccount(account) => {
                let next = Page::Account(account.clone());
                if self.page != next {
                    let prev = std::mem::replace(&mut self.page, next);
                    self.back_stack.push(prev);
                    cx.notify();
                }
            }
        }
    }

    fn open_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.back_stack.clear();
        if self.page != page {
            self.page = page;
            cx.notify();
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(prev) = self.back_stack.pop() {
            self.page = prev;
            cx.notify();
        }
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| session.reload(cx));
    }

    fn render_sidebar_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let can_open = self.path_opener.is_some();
        let file_label: SharedString = self
            .ledger_name
            .clone()
            .unwrap_or_else(|| "Select ledger…".into());
        let badge = div()
            .flex()
            .items_center()
            .justify_center()
            .size_8()
            .flex_shrink_0()
            .rounded(cx.theme().radius)
            .bg(cx.theme().sidebar_primary)
            .text_color(cx.theme().sidebar_primary_foreground)
            .child(Icon::new(IconName::BookOpen));

        if !can_open {
            return SidebarHeader::new()
                .child(badge)
                .child(
                    div()
                        .v_flex()
                        .flex_1()
                        .overflow_hidden()
                        .child(div().font_semibold().child("Lumi"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(file_label),
                        ),
                )
                .into_any_element();
        }

        let aria_label: SharedString = match &self.ledger_name {
            Some(name) => format!("Root ledger {name}, click to select another file").into(),
            None => "Select root ledger file".into(),
        };
        let tooltip: SharedString = match &self.ledger_path {
            Some(path) => format!("Switch root ledger ({path})").into(),
            None => "Select root ledger file".into(),
        };
        Button::new("open-ledger")
            .ghost()
            .w_full()
            .h_auto()
            .p_2()
            .accessibility_label(aria_label)
            .tooltip_with_action(tooltip, &OpenLedger, Some(KEY_CONTEXT))
            .on_click(cx.listener(|this, _, window, cx| this.prompt_open_ledger(window, cx)))
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(badge)
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .overflow_hidden()
                            .child(
                                div()
                                    .font_semibold()
                                    .text_color(cx.theme().sidebar_foreground)
                                    .child("Lumi"),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .text_xs()
                                    .text_left()
                                    .text_color(cx.theme().blue)
                                    .when(self.ledger_name.is_some(), |this| {
                                        this.font_family(cx.theme().mono_font_family.clone())
                                    })
                                    .truncate()
                                    .child(file_label),
                            ),
                    )
                    .child(
                        Icon::new(IconName::FolderOpen)
                            .size_4()
                            .flex_shrink_0()
                            .text_color(cx.theme().blue),
                    ),
            )
            .into_any_element()
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Sidebar::new("lumi-sidebar")
            .w_48()
            .header(self.render_sidebar_header(cx))
            .child(
                SidebarGroup::new("Reports").child(SidebarMenu::new().children(
                    Page::sidebar().map(|page| {
                        let active = self.page == page;
                        SidebarMenuItem::new(page.title())
                            .icon(page.icon())
                            .active(active)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.open_page(page.clone(), cx)),
                            )
                    }),
                )),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (has_source, reloading) = {
            let session = self.session.read(cx);
            (session.has_source(), session.is_reloading())
        };
        let previous = self
            .page
            .account()
            .and_then(|_| self.back_stack.last())
            .map(|page| page.title());
        div()
            .h_flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_6()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .when_some(previous, |this, prev_title| {
                        this.child(
                            Button::new("go-back")
                                .outline()
                                .small()
                                .icon(IconName::ArrowLeft)
                                .label("Go back")
                                .tooltip_with_action(
                                    format!("Back to {prev_title}"),
                                    &GoBack,
                                    Some(KEY_CONTEXT),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.go_back(&GoBack, window, cx)
                                })),
                        )
                    })
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .min_w_0()
                            .truncate()
                            // An account name is an identifier, set in the
                            // monospace face wherever it appears.
                            .when(self.page.account().is_some(), |this| {
                                this.font_family(cx.theme().mono_font_family.clone())
                            })
                            .child(self.page.title()),
                    ),
            )
            .child(
                Button::new("refresh")
                    .ghost()
                    .icon(IconName::RefreshCw)
                    .label("Refresh")
                    .loading(reloading)
                    .disabled(!has_source || reloading)
                    .tooltip_with_action("Read the ledger again", &Refresh, Some(KEY_CONTEXT))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx)),
                    ),
            )
    }

    fn render_empty_ledger(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_open = self.path_opener.is_some();
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_6()
            .child(
                Icon::new(IconName::BookOpen)
                    .large()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().text_lg().font_semibold().child("No ledger open"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Select a Beancount root file to view its reports."),
            )
            .when(can_open, |this| {
                this.child(
                    Button::new("open-ledger-empty")
                        .primary()
                        .icon(IconName::FolderOpen)
                        .label("Open ledger…")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.prompt_open_ledger(window, cx)),
                        ),
                )
            })
    }
}

impl Focusable for LumiApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for LumiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (has_source, last_error) = {
            let session = self.session.read(cx);
            (session.has_source(), session.last_error().cloned())
        };
        let open_error = self.open_error.clone();
        let content: AnyElement = if has_source {
            let page = self.page.clone();
            self.report(&page, window, cx).into_any_element()
        } else {
            self.render_empty_ledger(cx).into_any_element()
        };
        div()
            .h_flex()
            .items_stretch()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::on_open_ledger))
            .on_action(cx.listener(Self::refresh))
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_header(cx))
                    .when_some(open_error, |this, message| {
                        this.child(
                            Alert::error("open-ledger-failed", message)
                                .title("Couldn't open the file picker")
                                .banner(),
                        )
                    })
                    .when_some(last_error, |this, message| {
                        this.child(
                            Alert::error("reload-failed", message)
                                .title("Couldn't reload the ledger")
                                .banner(),
                        )
                    })
                    .child(div().flex_1().min_h_0().child(content)),
            )
    }
}

#[cfg(test)]
#[path = "app_test.rs"]
mod tests;

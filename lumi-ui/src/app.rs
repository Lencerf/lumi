use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;
#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::input::Search;
use gpui_kit::component::menu::AppMenuBar;
use gpui_kit::component::sidebar::{
    Sidebar, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, TitleBar,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, ExternalPaths, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, MouseButton, NavigationDirection,
    ParentElement as _, PathPromptOptions, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TestSupportExt as _, Window,
    actions, div, px,
};
#[cfg(not(target_family = "wasm"))]
use gpui_kit::{
    Decorations, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowDecorations,
    WindowOptions, ease_in_out, size,
};

use crate::entry_editor::{EntryEditorLayer, OpenSource};
use crate::errors_report::ErrorsReport;
use crate::holdings_report::HoldingsReport;
use crate::journal_report::JournalReport;
use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::source::LedgerSource;
use crate::source_editor::SourceEditor;
#[cfg(not(target_family = "wasm"))]
use crate::style::report_horizontal_overhead;
use crate::trie_report::TrieReport;

actions!(
    lumi_ui,
    [
        CloseWindow,
        GoBack,
        GoForward,
        Hide,
        HideOthers,
        Minimize,
        OpenLedger,
        Quit,
        Refresh,
        ShowAll,
        ShowBalanceSheet,
        ShowEditor,
        ShowErrors,
        ShowHoldings,
        ShowIncome,
        ShowJournal,
        ToggleFullScreen,
        Zoom,
    ]
);

/// The key context of the application view; its key bindings apply within it.
pub(crate) const KEY_CONTEXT: &str = "LumiApp";

/// Maximum number of previously visited views remembered by Back/Forward history.
pub(crate) const MAX_HISTORY: usize = 10;

const SHOW_TITLE_BAR: bool = !cfg!(target_os = "macos") && !cfg!(target_family = "wasm");

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
    /// Up to [`MAX_HISTORY`] previously visited views, oldest first.
    back_stack: VecDeque<Page>,
    /// Views stepped back from, ready to revisit via `GoForward`.
    forward_stack: Vec<Page>,
    /// Reports are created the first time they are opened and kept, so that
    /// returning to one does not query the ledger again; cleared when
    /// switching to another root file.
    reports: HashMap<Page, Report>,
    editor_layer: Entity<EntryEditorLayer>,
    menu_bar: Option<Entity<AppMenuBar>>,
    focus_handle: FocusHandle,
    pending_initial_fit: bool,
    user_resized_width: bool,
    pending_auto_widths: VecDeque<Pixels>,
    last_observed_width: Pixels,
    #[cfg(not(target_family = "wasm"))]
    width_animation: Option<WidthAnimation>,
    #[cfg(not(target_family = "wasm"))]
    next_width_animation_id: u64,
    _open_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// Active smooth window-width transition toward `target_width`.
#[cfg(not(target_family = "wasm"))]
struct WidthAnimation {
    id: u64,
    target_width: Pixels,
    _task: Task<()>,
}

/// The view behind a page, and the subscriptions that route its requests and
/// content-width updates to the application.
struct Report {
    view: AnyView,
    _subscriptions: Vec<Subscription>,
}

impl LumiApp {
    #[cfg(not(target_family = "wasm"))]
    pub(crate) const AUTO_WIDTH_ANIMATION_DURATION: Duration = Duration::from_millis(500);
    #[cfg(not(target_family = "wasm"))]
    const AUTO_WIDTH_FRAME_INTERVAL: Duration = Duration::from_millis(16);
    #[cfg(not(target_family = "wasm"))]
    const MIN_AUTO_WINDOW_WIDTH: Pixels = px(680.);

    /// Builds platform-appropriate [`WindowOptions`] for a native [`LumiApp`]
    /// window.
    ///
    /// macOS uses native AppKit window chrome and the system menu bar; Linux
    /// and Windows render a client-side [`TitleBar`] with an [`AppMenuBar`],
    /// window dragging, and min/max/close controls.
    #[cfg(not(target_family = "wasm"))]
    pub fn window_options(title: SharedString, bounds: WindowBounds) -> WindowOptions {
        let titlebar = if cfg!(target_os = "macos") {
            TitlebarOptions {
                title: Some(title),
                ..Default::default()
            }
        } else {
            TitlebarOptions {
                title: Some(title),
                ..TitleBar::title_bar_options()
            }
        };
        WindowOptions {
            titlebar: Some(titlebar),
            window_bounds: Some(bounds),
            window_min_size: Some(size(px(520.), px(360.))),
            app_id: Some("io.github.lencerf.lumi".into()),
            app_owns_titlebar_drag: !cfg!(target_os = "macos"),
            window_decorations: cfg!(target_os = "linux").then_some(WindowDecorations::Client),
            window_background: if cfg!(target_os = "linux") {
                WindowBackgroundAppearance::Transparent
            } else {
                WindowBackgroundAppearance::Opaque
            },
            ..Default::default()
        }
    }

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
        let editor_layer = EntryEditorLayer::register(window, cx);
        let menu_bar = SHOW_TITLE_BAR.then(|| AppMenuBar::new(cx));
        let last_observed_width = window.viewport_size().width;
        let _subscriptions = vec![
            cx.observe(&session, |_, _, cx| cx.notify()),
            cx.subscribe_in(&editor_layer, window, Self::on_open_source),
            cx.observe_window_bounds(window, Self::on_window_bounds_changed),
        ];
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        Self {
            session,
            ledger_name: None,
            ledger_path: None,
            path_opener: default_path_opener(),
            open_error: None,
            page,
            back_stack: VecDeque::new(),
            forward_stack: Vec::new(),
            reports: HashMap::new(),
            editor_layer,
            menu_bar,
            focus_handle,
            pending_initial_fit: false,
            user_resized_width: false,
            pending_auto_widths: VecDeque::new(),
            last_observed_width,
            #[cfg(not(target_family = "wasm"))]
            width_animation: None,
            #[cfg(not(target_family = "wasm"))]
            next_width_animation_id: 0,
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
            None => window.set_window_title(&this.window_title()),
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

    pub fn ledger_path(&self) -> Option<&SharedString> {
        self.ledger_path.as_ref()
    }

    pub fn session(&self) -> &Entity<LedgerSession> {
        &self.session
    }

    /// The hierarchy report shown, if the page is [`Page::BalanceSheet`] or
    /// [`Page::Income`].
    pub fn trie_report(&self) -> Option<Entity<TrieReport>> {
        self.reports
            .get(&self.page)?
            .view
            .clone()
            .downcast::<TrieReport>()
            .ok()
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

    /// The holdings report shown, if the page is [`Page::Holdings`].
    pub fn holdings(&self) -> Option<Entity<HoldingsReport>> {
        self.reports
            .get(&self.page)?
            .view
            .clone()
            .downcast::<HoldingsReport>()
            .ok()
    }

    /// The errors report shown, if the page is [`Page::Errors`].
    pub fn errors(&self) -> Option<Entity<ErrorsReport>> {
        self.reports
            .get(&self.page)?
            .view
            .clone()
            .downcast::<ErrorsReport>()
            .ok()
    }

    /// The built-in source editor shown, if the page is [`Page::Editor`].
    pub fn editor(&self) -> Option<Entity<SourceEditor>> {
        self.reports
            .get(&self.page)?
            .view
            .clone()
            .downcast::<SourceEditor>()
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
        cx.add_recent_document(path);
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
        self.ledger_name = Some(ledger_name);
        window.set_window_title(&self.window_title());
        self.ledger_path = ledger_path;
        self.open_error = None;
        self.back_stack.clear();
        self.forward_stack.clear();
        self.reports.clear();
        self.editor_layer
            .update(cx, |layer, cx| layer.close(window, cx));
        // When switching ledgers, an account page belonged to the previous
        // ledger's chart of accounts.
        if had_source && self.page.account().is_some() {
            self.page = Page::BalanceSheet;
        }
        #[cfg(not(target_family = "wasm"))]
        {
            self.width_animation = None;
        }
        self.pending_initial_fit = true;
        self.user_resized_width = false;
        self.session
            .update(cx, |session, cx| session.set_source(source, cx));
        let page = self.page.clone();
        self.report(&page, window, cx);
        self.sync_window_width(window, cx);
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
                    _subscriptions: vec![
                        cx.subscribe_in(&view, window, Self::on_report_event),
                        cx.observe_in(&view, window, Self::on_report_updated),
                    ],
                    view: view.into(),
                }
            }
            Page::Journal | Page::Account(_) => {
                let view = cx.new(|cx| JournalReport::new(page.clone(), session, window, cx));
                Report {
                    _subscriptions: vec![
                        cx.subscribe_in(&view, window, Self::on_report_event),
                        cx.observe_in(&view, window, Self::on_report_updated),
                    ],
                    view: view.into(),
                }
            }
            Page::Holdings => {
                let view = cx.new(|cx| HoldingsReport::new(session, window, cx));
                Report {
                    _subscriptions: vec![
                        cx.subscribe_in(&view, window, Self::on_report_event),
                        cx.observe_in(&view, window, Self::on_report_updated),
                    ],
                    view: view.into(),
                }
            }
            Page::Errors => {
                let view = cx.new(|cx| ErrorsReport::new(session, window, cx));
                Report {
                    _subscriptions: vec![
                        cx.subscribe_in(&view, window, Self::on_report_event),
                        cx.observe_in(&view, window, Self::on_report_updated),
                    ],
                    view: view.into(),
                }
            }
            Page::Editor => {
                let view = cx.new(|cx| SourceEditor::new(session, window, cx));
                Report {
                    _subscriptions: vec![cx.observe_in(&view, window, Self::on_report_updated)],
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
        _: &Entity<T>,
        event: &ReportEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ReportEvent::OpenAccount(account) => {
                self.navigate_to(Page::Account(account.clone()), window, cx);
            }
            ReportEvent::OpenAccountEntry { account, source } => {
                self.open_account_entry(account.clone(), source.clone(), window, cx);
            }
            ReportEvent::ShowInJournal(source) => {
                self.open_journal_entry(source.clone(), window, cx);
            }
            ReportEvent::OpenSource { file, line } => {
                self.open_source_location(file.clone(), *line, window, cx);
            }
        }
    }

    fn open_account_entry(
        &mut self,
        account: SharedString,
        source: lumi::Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = Page::Account(account);
        self.navigate_to(next.clone(), window, cx);
        if let Ok(journal) = self.report(&next, window, cx).downcast::<JournalReport>() {
            journal.update(cx, |journal, cx| {
                journal.show_entry_in_context(source, window, cx)
            });
        }
    }

    fn open_journal_entry(
        &mut self,
        source: lumi::Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_to(Page::Journal, window, cx);
        if let Ok(journal) = self
            .report(&Page::Journal, window, cx)
            .downcast::<JournalReport>()
        {
            journal.update(cx, |journal, cx| {
                journal.show_entry_in_context(source, window, cx)
            });
        }
    }

    fn on_open_source(
        &mut self,
        _: &Entity<EntryEditorLayer>,
        event: &OpenSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_source_location(event.file.clone(), event.line, window, cx);
    }

    fn open_source_location(
        &mut self,
        file: SharedString,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_to(Page::Editor, window, cx);
        if let Ok(editor) = self
            .report(&Page::Editor, window, cx)
            .downcast::<SourceEditor>()
        {
            editor.update(cx, |editor, cx| {
                editor.open_location(file, line, window, cx)
            });
        }
    }

    fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_to(page, window, cx);
    }

    fn navigate_to(&mut self, next: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == next {
            return;
        }
        let has_source = self.session.read(cx).has_source();
        let prev = std::mem::replace(&mut self.page, next);
        if has_source {
            self.forward_stack.clear();
            if self.back_stack.len() == MAX_HISTORY {
                self.back_stack.pop_front();
            }
            self.back_stack.push_back(prev);
        }
        self.activate_current_view(window, cx);
    }

    fn go_back(&mut self, _: &GoBack, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_layer.read(cx).is_open() || !self.session.read(cx).has_source() {
            return;
        }
        if let Some(prev) = self.back_stack.pop_back() {
            let current = std::mem::replace(&mut self.page, prev);
            self.forward_stack.push(current);
            self.activate_current_view(window, cx);
        }
    }

    fn go_forward(&mut self, _: &GoForward, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_layer.read(cx).is_open() || !self.session.read(cx).has_source() {
            return;
        }
        if let Some(next) = self.forward_stack.pop() {
            let current = std::mem::replace(&mut self.page, next);
            if self.back_stack.len() == MAX_HISTORY {
                self.back_stack.pop_front();
            }
            self.back_stack.push_back(current);
            self.activate_current_view(window, cx);
        }
    }

    fn activate_current_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(not(target_family = "wasm"))]
        {
            self.width_animation = None;
        }
        self.pending_initial_fit = false;
        self.user_resized_width = false;
        if self.session.read(cx).has_source() {
            let page = self.page.clone();
            self.report(&page, window, cx);
            self.sync_window_width(window, cx);
        }
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn on_report_updated<T: 'static>(
        &mut self,
        _: Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_window_width(window, cx);
        cx.notify();
    }

    fn on_window_bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let width = window.viewport_size().width;
        if let Some(pos) = self
            .pending_auto_widths
            .iter()
            .rposition(|&expected| (width - expected).abs() <= px(2.))
        {
            self.pending_auto_widths.drain(..=pos);
            self.last_observed_width = width;
            cx.notify();
            return;
        }
        if (width - self.last_observed_width).abs() <= px(1.) {
            return;
        }
        self.last_observed_width = width;
        self.pending_auto_widths.clear();
        #[cfg(not(target_family = "wasm"))]
        {
            self.width_animation = None;
        }
        self.user_resized_width = true;
        self.pending_initial_fit = false;
        cx.notify();
    }

    #[cfg(not(target_family = "wasm"))]
    fn preferred_report_content_width(&self, window: &Window, cx: &mut App) -> Option<Pixels> {
        if !self.session.read(cx).has_source() {
            return None;
        }
        let view = self.reports.get(&self.page)?.view.clone();
        if let Ok(trie) = view.clone().downcast::<TrieReport>() {
            trie.read(cx).preferred_content_width(window, cx)
        } else if let Ok(journal) = view.clone().downcast::<JournalReport>() {
            journal.update(cx, |r, cx| r.preferred_content_width(window, cx))
        } else if let Ok(holdings) = view.clone().downcast::<HoldingsReport>() {
            holdings.update(cx, |r, cx| r.preferred_content_width(window, cx))
        } else if let Ok(errors) = view.clone().downcast::<ErrorsReport>() {
            errors.update(cx, |r, cx| r.preferred_content_width(window, cx))
        } else if let Ok(editor) = view.downcast::<SourceEditor>() {
            editor.read(cx).preferred_content_width(window, cx)
        } else {
            None
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn apply_auto_window_width(
        &mut self,
        new_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rounded_width = px(f32::from(new_width).round());
        if self
            .pending_auto_widths
            .back()
            .is_some_and(|&last| (last - rounded_width).abs() < px(0.5))
        {
            return;
        }
        if (window.viewport_size().width - rounded_width).abs() < px(0.5)
            && self.pending_auto_widths.is_empty()
        {
            return;
        }
        if self.pending_auto_widths.len() >= 128 {
            self.pending_auto_widths.pop_front();
        }
        self.pending_auto_widths.push_back(rounded_width);
        let new_size = size(rounded_width, window.viewport_size().height);
        window.resize(new_size);
        #[cfg(test)]
        if window.bounds().size == new_size && window.viewport_size() != new_size {
            window.defer(cx, |window, cx| {
                window.bounds_changed(cx);
            });
        }
        #[cfg(not(test))]
        let _ = cx;
    }

    #[cfg(not(target_family = "wasm"))]
    fn start_width_animation(
        &mut self,
        start_width: Pixels,
        target_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target_width = px(f32::from(target_width).round());
        if cx.reduce_motion() || (target_width - start_width).abs() <= px(1.) {
            self.width_animation = None;
            self.apply_auto_window_width(target_width, window, cx);
            return;
        }
        self.next_width_animation_id = self.next_width_animation_id.wrapping_add(1);
        let id = self.next_width_animation_id;
        let start_px = f32::from(start_width);
        let delta_px = f32::from(target_width) - start_px;
        let start_time = cx.background_executor().now();
        let duration = Self::AUTO_WIDTH_ANIMATION_DURATION;
        let frame_interval = Self::AUTO_WIDTH_FRAME_INTERVAL;

        let task = cx.spawn_in(window, async move |this, cx| {
            let mut elapsed = Duration::ZERO;
            loop {
                let remaining = duration.saturating_sub(elapsed);
                let sleep = remaining.min(frame_interval).max(Duration::from_millis(1));
                cx.background_executor().timer(sleep).await;
                elapsed = cx
                    .background_executor()
                    .now()
                    .saturating_duration_since(start_time);
                let done = elapsed >= duration;
                let next_width = if done {
                    target_width
                } else {
                    let raw_t = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
                    let eased_t = ease_in_out(raw_t);
                    px((start_px + delta_px * eased_t).round())
                };
                let keep_going = this
                    .update_in(cx, |this, window, cx| {
                        if !this
                            .width_animation
                            .as_ref()
                            .is_some_and(|anim| anim.id == id)
                        {
                            return false;
                        }
                        if this.user_resized_width
                            || window.is_fullscreen()
                            || window.is_maximized()
                        {
                            this.width_animation = None;
                            return false;
                        }
                        this.apply_auto_window_width(next_width, window, cx);
                        if done {
                            this.width_animation = None;
                            return false;
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        });

        self.width_animation = Some(WidthAnimation {
            id,
            target_width,
            _task: task,
        });
    }

    fn sync_window_width(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_family = "wasm")]
        {
            let _ = (window, cx);
        }
        #[cfg(not(target_family = "wasm"))]
        {
            if self.user_resized_width || window.is_fullscreen() || window.is_maximized() {
                return;
            }
            if matches!(
                window.window_decorations(),
                Decorations::Client { tiling } if tiling.left && tiling.right
            ) {
                return;
            }
            let Some(content_width) = self.preferred_report_content_width(window, cx) else {
                return;
            };
            let raw_desired =
                px(f32::from(report_horizontal_overhead(window) + content_width).ceil());
            let max_width = window
                .display(cx)
                .map(|display| {
                    let w = display.bounds().size.width;
                    #[cfg(test)]
                    {
                        w
                    }
                    #[cfg(not(test))]
                    {
                        (w - px(40.)).max(Self::MIN_AUTO_WINDOW_WIDTH)
                    }
                })
                .unwrap_or(px(3840.));
            let desired_width =
                px(f32::from(raw_desired.clamp(Self::MIN_AUTO_WINDOW_WIDTH, max_width)).round());
            let actual_width = self
                .pending_auto_widths
                .back()
                .copied()
                .unwrap_or_else(|| window.viewport_size().width);
            let effective_width = self
                .width_animation
                .as_ref()
                .map(|anim| anim.target_width)
                .unwrap_or(actual_width);
            let should_resize = if self.pending_initial_fit {
                self.pending_initial_fit = false;
                (desired_width - effective_width).abs() > px(1.)
            } else {
                desired_width > effective_width + px(1.)
            };
            if should_resize {
                self.start_width_animation(actual_width, desired_width, window, cx);
            }
        }
    }

    fn on_search(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_layer.read(cx).is_open() || !self.session.read(cx).has_source() {
            return;
        }
        let page = self.page.clone();
        let view = self.report(&page, window, cx);
        if let Ok(journal) = view.clone().downcast::<JournalReport>() {
            journal.update(cx, |journal, cx| journal.focus_search(window, cx));
        } else if let Ok(holdings) = view.clone().downcast::<HoldingsReport>() {
            holdings.update(cx, |holdings, cx| holdings.focus_search(window, cx));
        } else if let Ok(editor) = view.downcast::<SourceEditor>() {
            editor.update(cx, |editor, cx| editor.focus_search(window, cx));
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
        let error_count = self.session.read(cx).error_count();
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
                            .when(page == Page::Errors && error_count > 0, |item| {
                                let count_text: SharedString = error_count.to_string().into();
                                item.suffix(move |_, cx| {
                                    div()
                                        .id("sidebar-error-count")
                                        .test_support()
                                        .aria_label(count_text.clone())
                                        .flex_shrink_0()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_full()
                                        .bg(cx.theme().red.opacity(0.14))
                                        .text_xs()
                                        .font_semibold()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(cx.theme().red)
                                        .child(count_text.clone())
                                })
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_page(page.clone(), window, cx)
                            }))
                    }),
                )),
            )
    }

    fn render_history_buttons(&self, has_source: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let prev_title = has_source
            .then(|| self.back_stack.back().map(Page::title))
            .flatten();
        let next_title = has_source
            .then(|| self.forward_stack.last().map(Page::title))
            .flatten();

        div()
            .id("history-nav")
            .h_flex()
            .items_center()
            .flex_shrink_0()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .child(
                Button::new("go-back")
                    .ghost()
                    .small()
                    .compact()
                    .rounded(ButtonRounded::None)
                    .icon(IconName::ArrowLeft)
                    .accessibility_label("Back")
                    .disabled(prev_title.is_none())
                    .when_some(prev_title, |btn, title| {
                        btn.tooltip_with_action(
                            format!("Back to {title}"),
                            &GoBack,
                            Some(KEY_CONTEXT),
                        )
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.go_back(&GoBack, window, cx))),
            )
            .child(div().w(px(1.)).self_stretch().bg(cx.theme().border))
            .child(
                Button::new("go-forward")
                    .ghost()
                    .small()
                    .compact()
                    .rounded(ButtonRounded::None)
                    .icon(IconName::ArrowRight)
                    .accessibility_label("Forward")
                    .disabled(next_title.is_none())
                    .when_some(next_title, |btn, title| {
                        btn.tooltip_with_action(
                            format!("Forward to {title}"),
                            &GoForward,
                            Some(KEY_CONTEXT),
                        )
                    })
                    .on_click(
                        cx.listener(|this, _, window, cx| this.go_forward(&GoForward, window, cx)),
                    ),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (has_source, reloading) = {
            let session = self.session.read(cx);
            (session.has_source(), session.is_reloading())
        };
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
                    .child(self.render_history_buttons(has_source, cx))
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

    fn window_title(&self) -> SharedString {
        match &self.ledger_name {
            Some(name) => format!("{name} — Lumi").into(),
            None => "Lumi".into(),
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> Option<TitleBar> {
        let menu_bar = self.menu_bar.as_ref()?;
        let title = self.window_title();
        Some(
            TitleBar::new()
                .relative()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .h_full()
                        .flex_1()
                        .min_w_0()
                        .child(menu_bar.clone()),
                )
                .child(
                    div()
                        .id("window-title")
                        .test_support()
                        .aria_label(title.clone())
                        .flex_shrink_0()
                        .px_3()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(title),
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
        let main_column = div()
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
            .child(div().flex_1().min_h_0().child(content));

        div()
            .relative()
            .v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                if let Some(path) = paths.paths().first() {
                    this.open_path(path, window, cx);
                }
            }))
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| this.go_back(&GoBack, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| this.go_forward(&GoForward, window, cx)),
            )
            .on_action(cx.listener(Self::on_open_ledger))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::on_search))
            .on_action(cx.listener(|this, _: &ShowBalanceSheet, window, cx| {
                this.open_page(Page::BalanceSheet, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowIncome, window, cx| {
                this.open_page(Page::Income, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowJournal, window, cx| {
                this.open_page(Page::Journal, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowHoldings, window, cx| {
                this.open_page(Page::Holdings, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowErrors, window, cx| {
                this.open_page(Page::Errors, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowEditor, window, cx| {
                this.open_page(Page::Editor, window, cx)
            }))
            .children(self.render_title_bar(cx))
            .child(
                div()
                    .h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.render_sidebar(cx))
                    .child(main_column),
            )
            .child(self.editor_layer.clone())
    }
}

#[cfg(test)]
#[path = "app_test.rs"]
mod tests;

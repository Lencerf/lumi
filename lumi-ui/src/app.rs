use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
    SidebarToggleButton,
};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window, actions, div,
};

use crate::errors_report::ErrorsReport;
use crate::holdings_report::HoldingsReport;
use crate::journal_report::JournalReport;
use crate::navigation::{Page, ReportEvent};
use crate::session::LedgerSession;
use crate::source::LedgerSource;
use crate::trie_report::TrieReport;

actions!(lumi_ui, [Refresh]);

/// The key context of the application view; its key bindings apply within it.
pub(crate) const KEY_CONTEXT: &str = "LumiApp";

/// The application window: a sidebar of reports beside the selected report,
/// with the ledger-level commands in the header.
pub struct LumiApp {
    session: Entity<LedgerSession>,
    ledger_name: SharedString,
    page: Page,
    /// Reports are created the first time they are opened and kept, so that
    /// returning to one does not query the ledger again.
    reports: HashMap<Page, Report>,
    sidebar_collapsed: bool,
    focus_handle: FocusHandle,
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
        let session = cx.new(|_| LedgerSession::new(source));
        session.update(cx, |session, cx| session.reload(cx));
        let _subscriptions = vec![cx.observe(&session, |_, _, cx| cx.notify())];
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let mut this = Self {
            session,
            ledger_name: ledger_name.into(),
            page: page.clone(),
            reports: HashMap::new(),
            sidebar_collapsed: false,
            focus_handle,
            _subscriptions,
        };
        this.report(&page, window, cx);
        this
    }

    pub fn page(&self) -> &Page {
        &self.page
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
            ReportEvent::OpenAccount(account) => self.open_page(Page::Account(account.clone()), cx),
        }
    }

    fn open_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            cx.notify();
        }
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| session.reload(cx));
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.sidebar_collapsed;
        Sidebar::new("lumi-sidebar")
            .collapsible(SidebarCollapsible::Icon)
            .collapsed(collapsed)
            .w_64()
            .header(
                SidebarHeader::new()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size_8()
                            .flex_shrink_0()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().sidebar_primary)
                            .text_color(cx.theme().sidebar_primary_foreground)
                            .child(Icon::new(IconName::BookOpen)),
                    )
                    .when(!collapsed, |this| {
                        this.child(
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
                                        .child(self.ledger_name.clone()),
                                ),
                        )
                    }),
            )
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
        let reloading = self.session.read(cx).is_reloading();
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
                    .child(
                        SidebarToggleButton::new()
                            .collapsed(self.sidebar_collapsed)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
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
                    .disabled(reloading)
                    .tooltip_with_action("Read the ledger again", &Refresh, Some(KEY_CONTEXT))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx)),
                    ),
            )
    }
}

impl Focusable for LumiApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for LumiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page.clone();
        let report = self.report(&page, window, cx);
        let last_error = self.session.read(cx).last_error().cloned();
        div()
            .h_flex()
            .items_stretch()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::refresh))
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_header(cx))
                    .when_some(last_error, |this, message| {
                        this.child(
                            Alert::error("reload-failed", message)
                                .title("Couldn't reload the ledger")
                                .banner(),
                        )
                    })
                    .child(div().flex_1().min_h_0().child(report)),
            )
    }
}

use std::fs;
use std::path::{Path, PathBuf};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, ElementId, Entity, SharedString, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};

use crate::app::{LumiApp, ledger_name_for_path};
use crate::navigation::Page;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../lumi/src/query_test.bean");

const VAULT_LEDGER: &str = "\
2025-01-01 open Assets:Vault
2025-01-01 open Equity:Opening

2025-01-02 * \"Seed vault\"
  Assets:Vault              500.00 USD
  Equity:Opening           -500.00 USD
";

struct TempLedger(PathBuf);

impl TempLedger {
    fn new(name: &str, contents: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("lumi-app-test-{}-{name}.bean", std::process::id()));
        fs::write(&path, contents).unwrap();
        Self(path)
    }

    fn file_name(&self) -> SharedString {
        ledger_name_for_path(&self.0)
    }
}

impl Drop for TempLedger {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn open_app(
    path: Option<&Path>,
    page: Page,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<LumiApp>) {
    cx.update(crate::init);
    let path = path.map(Path::to_path_buf);
    let (handle, app) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| LumiApp::new_local(path.as_deref(), page, window, cx)),
        )
        .expect("open test window")
    });
    cx.run_until_parked();
    (handle, app)
}

#[test]
fn test_ledger_name_for_path() {
    assert_eq!(
        ledger_name_for_path(Path::new("/ledgers/2026/main.bean")),
        SharedString::from("main.bean")
    );
    assert_eq!(
        ledger_name_for_path(Path::new("main.bean")),
        SharedString::from("main.bean")
    );
    assert_eq!(
        ledger_name_for_path(Path::new("/")),
        SharedString::from("/")
    );
}

#[gpui_kit::test]
fn test_starting_without_a_ledger_then_selecting_one_from_the_empty_state(cx: &mut TestAppContext) {
    let (handle, app) = open_app(None, Page::BalanceSheet, cx);

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ledger_name(), None);
        let session = app.session().read(cx);
        assert!(!session.has_source());
        assert!(!session.is_loaded());
        assert_eq!(session.revision(), 0);
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("refresh").disabled(), Some(true));
        assert!(window.try_find("open-ledger").is_some());
        window.click("open-ledger-empty", cx);
    })
    .unwrap();

    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files);
        assert!(!options.directories);
        assert!(!options.multiple);
        Some(vec![PathBuf::from(FIXTURE)])
    });
    cx.run_until_parked();

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        let session = app.session().read(cx);
        assert!(session.has_source());
        assert!(session.is_loaded());
        assert_eq!(session.revision(), 1);
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("open-ledger-empty").is_none());
        assert_ne!(window.find("refresh").disabled(), Some(true));
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Cash"))
                .is_some()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn test_clicking_the_root_file_in_the_sidebar_switches_ledgers(cx: &mut TestAppContext) {
    let vault = TempLedger::new("vault", VAULT_LEDGER);
    let (handle, app) = open_app(
        Some(Path::new(FIXTURE)),
        Page::Account("Assets:Cash".into()),
        cx,
    );

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        assert_eq!(app.page(), &Page::Account("Assets:Cash".into()));
        assert_eq!(app.session().read(cx).revision(), 1);
    });

    // Cancelling the file picker leaves the current ledger and page untouched.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("open-ledger", cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        assert_eq!(app.page(), &Page::Account("Assets:Cash".into()));
        assert_eq!(app.session().read(cx).revision(), 1);
    });

    // Choosing another root file reloads into it and resets an account page to
    // the balance sheet of the new chart of accounts.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("open-ledger", cx);
    })
    .unwrap();
    let vault_path = vault.0.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![vault_path]));
    cx.run_until_parked();

    let expected_name = vault.file_name();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ledger_name(), Some(&expected_name));
        assert_eq!(app.page(), &Page::BalanceSheet);
        assert_eq!(app.session().read(cx).revision(), 2);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Vault"))
                .is_some()
        );
        assert!(
            window
                .try_find((ElementId::from("account"), "Assets:Cash"))
                .is_none()
        );
    })
    .unwrap();

    // Switching while on a report page keeps that page and loads the new
    // ledger's entries into it.
    cx.update_window(handle, |_, window, cx| {
        window.within("lumi-sidebar").click("0-0-2", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let report = app.read(cx).journal().expect("the journal is shown");
        assert_eq!(report.read(cx).total(cx), 1);
    });

    let shortcut = if cfg!(target_os = "macos") {
        "cmd-o"
    } else {
        "ctrl-o"
    };
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(shortcut, cx);
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from(FIXTURE)]));
    cx.run_until_parked();

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ledger_name(),
            Some(&SharedString::from("query_test.bean"))
        );
        assert_eq!(app.page(), &Page::Journal);
        let report = app.journal().expect("the journal is shown");
        assert_eq!(report.read(cx).total(cx), 9);
    });
}

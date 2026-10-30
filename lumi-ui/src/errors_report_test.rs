use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui_kit::{
    AppContext as _, Bounds, SharedString, TestAppContext, WindowBounds, WindowOptions, px, size,
};
use lumi::ErrorLevel;

use crate::LumiApp;
use crate::errors_report::common_dir;
use crate::navigation::Page;
use crate::source::LocalLedger;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lumi-errors-report-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_common_dir() {
    assert_eq!(common_dir([].into_iter()), "");
    assert_eq!(
        common_dir(["/home/me/ledger/main.bean"].into_iter()),
        "/home/me/ledger/"
    );
    assert_eq!(
        common_dir(["/home/me/ledger/main.bean", "/home/me/ledger/2024/01.bean"].into_iter()),
        "/home/me/ledger/"
    );
    assert_eq!(
        common_dir(
            [
                "/home/me/ledger/2024/01.bean",
                "/home/me/ledger/2025/01.bean"
            ]
            .into_iter()
        ),
        "/home/me/ledger/"
    );
    // A shared name prefix is not a shared directory.
    assert_eq!(
        common_dir(["/home/me/ledger/a.bean", "/home/me/ledgers/b.bean"].into_iter()),
        "/home/me/"
    );
    assert_eq!(common_dir(["a.bean", "b.bean"].into_iter()), "");
    assert_eq!(common_dir(["/a.bean", "/b.bean"].into_iter()), "/");
}

#[gpui_kit::test]
async fn test_sidebar_error_badge_and_clicking_error_location_jumps_to_editor_line(
    cx: &mut TestAppContext,
) {
    let dir = TempDir::new("badge-and-jump");
    let root = dir.write(
        "main.bean",
        "\
option \"operating_currency\" \"USD\"
plugin \"beancount.plugins.sellgains\"
2025-01-01 custom \"fava-option\" \"auto-reload\" \"True\"
include \"sub.bean\"
",
    );
    let sub = dir.write(
        "sub.bean",
        "\
2025-01-01 open Assets:Cash
2025-01-01 open Expenses:Food
* Section heading

2025-01-02 * \"Unbalanced lunch\"
  Expenses:Food   15.00 USD
  Assets:Cash    -10.00 USD
",
    );
    let sub_path: SharedString = sub.to_str().unwrap().to_string().into();

    cx.update(crate::init);
    let root_str = root.to_str().unwrap().to_string();
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
            move |window, cx| {
                cx.new(|cx| {
                    LumiApp::new(
                        Arc::new(LocalLedger::new(root_str)),
                        "main.bean",
                        Page::BalanceSheet,
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("open test window")
    });
    cx.run_until_parked();

    // Even on Balance Sheet before Errors has been opened, the sidebar "Errors"
    // item ("0-0-4") shows "2" for the 2 Error-level diagnostics (excluding Info).
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(app.read(cx).session().read(cx).error_count(), 2);
        assert_eq!(
            window.within("0-0-4").find("sidebar-error-count").label(),
            Some("2")
        );
        window.within("lumi-sidebar").click("0-0-4", cx);
    })
    .unwrap();
    cx.run_until_parked();

    // ErrorsReport lists the 2 Error rows first, followed by the 2 Info rows,
    // and clicking the 2nd error's location jumps to sub.bean line 5 in Editor.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let report = app.read(cx).errors().expect("errors report is active");
        let messages = report.read(cx).messages(cx);
        let levels: Vec<ErrorLevel> = messages.iter().map(|(lvl, _)| *lvl).collect();
        assert_eq!(
            levels,
            vec![
                ErrorLevel::Error,
                ErrorLevel::Error,
                ErrorLevel::Info,
                ErrorLevel::Info,
            ]
        );
        assert_eq!(
            window.find(("error-location", 1usize)).label(),
            Some("Open sub.bean:5:12 in editor")
        );
        window.click(("error-location", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();

    // The UI switched to Editor on sub.bean with the cursor at line 5.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(app.read(cx).page(), &Page::Editor);
        let editor = app.read(cx).editor().expect("source editor is active");
        assert_eq!(editor.read(cx).selected_file(), Some(&sub_path));
        assert_eq!(editor.read(cx).cursor_line(cx), 5);

        // Fix both errors in sub.bean and save.
        let state = editor.read(cx).editor_state().clone();
        state.update(cx, |state, cx| {
            state.set_value(
                "\
2025-01-01 open Assets:Cash
2025-01-01 open Expenses:Food

2025-01-02 * \"Balanced lunch\"
  Expenses:Food   15.00 USD
  Assets:Cash    -15.00 USD
",
                window,
                cx,
            );
        });
        window.render_frame(cx);
        window.click("save-source", cx);
    })
    .unwrap();

    cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
        app.read(cx).session().read(cx).revision() == 2
    })
    .await;
    cx.run_until_parked();

    // Only Info diagnostics remain (plugin + custom), so the red sidebar error
    // count disappears completely.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(app.read(cx).session().read(cx).error_count(), 0);
        assert!(window.try_find("sidebar-error-count").is_none());
    })
    .unwrap();
}

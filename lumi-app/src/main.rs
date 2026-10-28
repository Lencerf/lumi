//! The native lumi application: `lumi-app <ledger.bean> [--page <page>]`.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use gpui_kit::{
    AppContext as _, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use lumi_ui::source::LocalLedger;
use lumi_ui::{LumiApp, LumiAssets, Page};

const USAGE: &str = "usage: lumi-app <ledger.bean> [--page <page>]

  --page  the page to open: balance-sheet (default), income, journal,
          holdings, errors, or account:<name>";

fn main() -> ExitCode {
    let Some((path, page)) = parse_args(std::env::args().skip(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let ledger_name: SharedString = Path::new(&path)
        .file_name()
        .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned())
        .into();
    let title: SharedString = format!("{ledger_name} — Lumi").into();

    gpui_kit::application()
        .with_assets(LumiAssets)
        .run(move |cx| {
            lumi_ui::init(cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(title),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(800.)),
                    cx,
                ))),
                window_min_size: Some(size(px(640.), px(400.))),
                ..Default::default()
            };
            let source = Arc::new(LocalLedger::new(path));
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| LumiApp::new(source, ledger_name, page, window, cx))
            })
            .expect("failed to open the lumi window");
            cx.activate(true);
        });
    ExitCode::SUCCESS
}

/// The ledger path and the page to open, or `None` if the arguments are not
/// `<ledger.bean> [--page <page>]`.
fn parse_args(args: impl Iterator<Item = String>) -> Option<(String, Page)> {
    let mut path = None;
    let mut page = Page::BalanceSheet;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if arg == "--page" {
            page = Page::parse(&args.next()?)?;
        } else if let Some(name) = arg.strip_prefix("--page=") {
            page = Page::parse(name)?;
        } else if path.is_none() {
            path = Some(arg);
        } else {
            return None;
        }
    }
    Some((path?, page))
}

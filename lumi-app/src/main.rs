//! The native lumi application: `lumi-app [<ledger.bean>] [--page <page>]`.

use std::path::PathBuf;
use std::process::ExitCode;

use gpui_kit::{
    AppContext as _, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use lumi_ui::{LumiApp, LumiAssets, Page, ledger_name_for_path};

const USAGE: &str = "usage: lumi-app [<ledger.bean>] [--page <page>]

  --page  the page to open: balance-sheet (default), income, journal,
          holdings, errors, or account:<name>
  -h, --help  print this help message";

#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    Run { path: Option<PathBuf>, page: Page },
    Help,
}

fn main() -> ExitCode {
    let (path, page) = match parse_args(std::env::args().skip(1)) {
        Ok(CliAction::Run { path, page }) => (path, page),
        Ok(CliAction::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(()) => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let title: SharedString = match &path {
        Some(path) => format!("{} — Lumi", ledger_name_for_path(path)).into(),
        None => "Lumi".into(),
    };

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
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| LumiApp::new_local(path.as_deref(), page, window, cx))
            })
            .expect("failed to open the lumi window");
            cx.activate(true);
        });
    ExitCode::SUCCESS
}

/// Parses `[<ledger.bean>] [--page <page>]` or `-h` / `--help`.
fn parse_args(args: impl Iterator<Item = String>) -> Result<CliAction, ()> {
    let mut path = None;
    let mut page = Page::BalanceSheet;
    let mut positional_only = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if !positional_only {
            if arg == "--" {
                positional_only = true;
                continue;
            }
            if arg == "-h" || arg == "--help" {
                return Ok(CliAction::Help);
            }
            if arg == "--page" {
                let value = args.next().ok_or(())?;
                page = Page::parse(&value).ok_or(())?;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--page=") {
                page = Page::parse(value).ok_or(())?;
                continue;
            }
            if arg.starts_with('-') {
                return Err(());
            }
        }
        if path.is_none() {
            path = Some(PathBuf::from(arg));
        } else {
            return Err(());
        }
    }
    Ok(CliAction::Run { path, page })
}

#[cfg(test)]
#[path = "main_test.rs"]
mod tests;

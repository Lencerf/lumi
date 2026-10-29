//! The native lumi application: `lumi-app [<ledger.bean>] [--page <page>]`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Bounds, Entity, SharedString, TitlebarOptions,
    WindowBounds, WindowOptions, px, size,
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

struct AppState {
    last_path: Option<PathBuf>,
    window_view: Option<(AnyWindowHandle, Entity<LumiApp>)>,
    async_app: Option<AsyncApp>,
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
    let state = Rc::new(RefCell::new(AppState {
        last_path: path,
        window_view: None,
        async_app: None,
    }));
    let open_urls_state = state.clone();
    let reopen_state = state.clone();

    let app = gpui_kit::application().with_assets(LumiAssets);
    app.on_open_urls(move |urls| {
        let Some(path) = urls.iter().find_map(|url| file_url_to_path(url)) else {
            return;
        };
        let async_app = {
            let mut state = open_urls_state.borrow_mut();
            state.last_path = Some(path.clone());
            state.async_app.clone()
        };
        if let Some(async_app) = async_app {
            let state = open_urls_state.clone();
            async_app
                .spawn(async move |cx| {
                    cx.update(|cx| open_path_in_app(&state, path, cx));
                })
                .detach();
        }
    });
    app.on_reopen(move |cx| {
        if cx.windows().is_empty() {
            open_main_window(&reopen_state, Page::BalanceSheet, cx);
        }
        cx.activate(true);
    });
    app.run(move |cx| {
        state.borrow_mut().async_app = Some(cx.to_async());
        lumi_ui::init(cx);
        open_main_window(&state, page, cx);
        cx.activate(true);
    });
    ExitCode::SUCCESS
}

fn open_path_in_app(state: &Rc<RefCell<AppState>>, path: PathBuf, cx: &mut App) {
    let existing = {
        let mut state = state.borrow_mut();
        state.last_path = Some(path.clone());
        state.window_view.clone()
    };
    if let Some((window, view)) = existing
        && window
            .update(cx, |_, window, cx| {
                view.update(cx, |app, cx| app.open_path(&path, window, cx));
            })
            .is_ok()
    {
        cx.activate(true);
        return;
    }
    open_main_window(state, Page::BalanceSheet, cx);
    cx.activate(true);
}

fn open_main_window(state: &Rc<RefCell<AppState>>, page: Page, cx: &mut App) {
    let path = state.borrow().last_path.clone();
    let title: SharedString = match &path {
        Some(path) => format!("{} — Lumi", ledger_name_for_path(path)).into(),
        None => "Lumi".into(),
    };
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
    let (window, view) = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| LumiApp::new_local(path.as_deref(), page, window, cx))
    })
    .expect("failed to open the lumi window");
    state.borrow_mut().window_view = Some((window, view.clone()));

    let observed_state = state.clone();
    cx.observe(&view, move |app, cx| {
        if let Some(ledger_path) = app.read(cx).ledger_path() {
            observed_state.borrow_mut().last_path = Some(PathBuf::from(ledger_path.as_ref()));
        }
    })
    .detach();
}

/// Converts a `file://` URL received from macOS LaunchServices into a local
/// filesystem path.
fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let encoded_path = rest.split(['?', '#']).next().unwrap_or(rest);
    if !encoded_path.starts_with('/') {
        return None;
    }

    let bytes = percent_decode(encoded_path.as_bytes())?;
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;

        Some(PathBuf::from(OsString::from_vec(bytes)))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes).ok().map(PathBuf::from)
    }
}

fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            let hi = hex_nibble(*input.get(i + 1)?)?;
            let lo = hex_nibble(*input.get(i + 2)?)?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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
            // macOS LaunchServices may pass a Process Serial Number argument
            // (`-psn_0_12345`) when launching a `.app` bundle.
            if arg.starts_with("-psn_") {
                continue;
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

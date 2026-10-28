//! The native lumi application: `lumi-app <ledger.bean>`.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use gpui_kit::{
    AppContext as _, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use lumi_ui::source::LocalLedger;
use lumi_ui::{LumiApp, LumiAssets};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        eprintln!("usage: lumi-app <ledger.bean>");
        return ExitCode::FAILURE;
    };
    let path = path.to_string_lossy().into_owned();
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
                    size(px(1200.), px(800.)),
                    cx,
                ))),
                window_min_size: Some(size(px(640.), px(400.))),
                ..Default::default()
            };
            let source = Arc::new(LocalLedger::new(path));
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| LumiApp::new(source, ledger_name, window, cx))
            })
            .expect("failed to open the lumi window");
            cx.activate(true);
        });
    ExitCode::SUCCESS
}

mod api;
mod app;
mod components;
mod route;

fn main() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Trace));
    yew::start_app::<app::App>();
}

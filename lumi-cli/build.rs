use std::env::var;
use std::fs::{copy, create_dir_all, rename};
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let wasm_path = PathBuf::from(var("CARGO_BIN_FILE_LUMI_WEB").unwrap());
    let out_dir = PathBuf::from(var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(var("CARGO_MANIFEST_DIR").unwrap());

    let wasm_name = wasm_path.file_stem().unwrap().to_str().unwrap();
    let site_dir = out_dir.join("site");
    let tmp_dir = out_dir.join("tmp");
    create_dir_all(&site_dir).unwrap();

    let mut cmd = Command::new("wasm-bindgen");
    cmd.args(["--target", "web"]);
    cmd.arg("--out-dir");
    cmd.arg(&tmp_dir);
    cmd.arg(&wasm_path);
    cmd.spawn().unwrap();
    rename(
        tmp_dir.join(format!("{wasm_name}.js")),
        site_dir.join("lumi-web.js"),
    )
    .unwrap();
    rename(
        tmp_dir.join(format!("{wasm_name}_bg.wasm")),
        site_dir.join("lumi-web_bg.wasm"),
    )
    .unwrap();

    copy(manifest_dir.join("index.html"), site_dir.join("index.html")).unwrap();
    copy(manifest_dir.join("style.css"), site_dir.join("style.css")).unwrap();

    println!("cargo:rerun-if-changed=style.css");
    println!("cargo:rerun-if-changed=index.html");
}

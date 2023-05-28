use std::process::Command;

fn main() {
    let profile = std::env::var("PROFILE").unwrap();
    let trunk_args = if profile == "release" {
        vec!["build", "--release"]
    } else {
        vec!["build"]
    };
    let status = Command::new("trunk")
        .args(&trunk_args)
        .current_dir("../lumi-web")
        .status()
        .unwrap();
    assert!(status.success());
    println!("cargo:rerun-if-changed=../lumi-web/src");
    println!("cargo:rerun-if-changed=../lumi-web/style.css");
    println!("cargo:rerun-if-changed=../lumi-web/index.html");
}

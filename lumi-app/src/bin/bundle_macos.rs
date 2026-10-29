//! Packages `lumi-app` into a standard macOS `Lumi.app` application bundle.
//!
//! Run `cargo bundle-macos` (or `cargo bundle-macos --out-dir /Applications`)
//! to compile `lumi-app` in release mode, generate `Lumi.icns` from
//! `macos/icon.svg`, populate `Contents/Info.plist`, and ad-hoc sign the
//! resulting bundle on macOS.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use resvg::{tiny_skia, usvg};

const BUNDLE_NAME: &str = "Lumi.app";
const EXECUTABLE_NAME: &str = "lumi-app";
const ICON_FILE_NAME: &str = "Lumi.icns";
const PKG_INFO: &[u8; 8] = b"APPL????";

const INFO_PLIST_TEMPLATE: &str = include_str!("../../macos/Info.plist");
const ICON_SVG: &str = include_str!("../../macos/icon.svg");

/// Standard Apple `.icns` PNG-encoded icon slots (`OSType`, pixel width/height).
const ICNS_SLOTS: &[([u8; 4], u32)] = &[
    (*b"icp4", 16),   // 16x16
    (*b"icp5", 32),   // 32x32
    (*b"icp6", 64),   // 64x64
    (*b"ic07", 128),  // 128x128
    (*b"ic08", 256),  // 256x256
    (*b"ic09", 512),  // 512x512
    (*b"ic10", 1024), // 1024x1024 (512x512@2x)
    (*b"ic11", 32),   // 16x16@2x
    (*b"ic12", 64),   // 32x32@2x
    (*b"ic13", 256),  // 128x128@2x
    (*b"ic14", 512),  // 256x256@2x
];

const USAGE: &str = "usage: cargo bundle-macos [--debug] [--target <triple>] [--out-dir <dir>]

Builds lumi-app and packages it into a standard macOS Lumi.app bundle.

  --debug             package a debug build instead of a release build
  --target <triple>   compile for a Cargo target triple (e.g. aarch64-apple-darwin)
  --out-dir <dir>     write Lumi.app into <dir> (default: target/<profile>/bundle/osx)
  -h, --help          print this help message";

#[derive(Debug, PartialEq, Eq)]
struct BundleOptions {
    release: bool,
    target: Option<String>,
    out_dir: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
enum BundleCli {
    Build(BundleOptions),
    Help,
}

fn main() -> ExitCode {
    let options = match parse_args(std::env::args().skip(1)) {
        Ok(BundleCli::Build(options)) => options,
        Ok(BundleCli::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("error: {err}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match build_and_bundle(&options) {
        Ok(bundle_path) => {
            println!("Packaged {}", bundle_path.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<BundleCli, String> {
    let mut release = true;
    let mut target = None;
    let mut out_dir = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        if arg == "-h" || arg == "--help" {
            return Ok(BundleCli::Help);
        }
        if arg == "--debug" {
            release = false;
            continue;
        }
        if arg == "--release" {
            release = true;
            continue;
        }
        if arg == "--target" {
            let value = args
                .next()
                .ok_or_else(|| "missing value for --target".to_string())?;
            target = Some(value);
            continue;
        }
        if let Some(value) = arg.strip_prefix("--target=") {
            if value.is_empty() {
                return Err("missing value for --target".to_string());
            }
            target = Some(value.to_string());
            continue;
        }
        if arg == "--out-dir" {
            let value = args
                .next()
                .ok_or_else(|| "missing value for --out-dir".to_string())?;
            out_dir = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = arg.strip_prefix("--out-dir=") {
            if value.is_empty() {
                return Err("missing value for --out-dir".to_string());
            }
            out_dir = Some(PathBuf::from(value));
            continue;
        }
        return Err(format!("unknown argument: {arg}"));
    }

    Ok(BundleCli::Build(BundleOptions {
        release,
        target,
        out_dir,
    }))
}

fn build_and_bundle(options: &BundleOptions) -> Result<PathBuf, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .ok_or_else(|| "lumi-app manifest directory has no workspace parent".to_string())?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(workspace_root)
        .args(["build", "-p", "lumi-app", "--bin", EXECUTABLE_NAME]);
    if options.release {
        cmd.arg("--release");
    }
    if let Some(triple) = &options.target {
        cmd.args(["--target", triple]);
    }
    let status = cmd
        .status()
        .map_err(|err| format!("failed to invoke cargo build: {err}"))?;
    if !status.success() {
        return Err(format!("cargo build exited with {status}"));
    }

    let target_root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));
    let profile_dir = if options.release { "release" } else { "debug" };
    let build_dir = match &options.target {
        Some(triple) => target_root.join(triple).join(profile_dir),
        None => target_root.join(profile_dir),
    };
    let binary_path = build_dir.join(EXECUTABLE_NAME);
    let out_dir = options
        .out_dir
        .clone()
        .unwrap_or_else(|| build_dir.join("bundle").join("osx"));

    let bundle_path = assemble_bundle(&binary_path, &out_dir, env!("CARGO_PKG_VERSION"))?;
    codesign_if_macos(&bundle_path)?;
    Ok(bundle_path)
}

fn assemble_bundle(binary_path: &Path, out_dir: &Path, version: &str) -> Result<PathBuf, String> {
    if !binary_path.is_file() {
        return Err(format!(
            "compiled binary not found at {}",
            binary_path.display()
        ));
    }

    let bundle_path = out_dir.join(BUNDLE_NAME);
    if bundle_path.exists() {
        fs::remove_dir_all(&bundle_path)
            .map_err(|err| format!("failed to clear existing {}: {err}", bundle_path.display()))?;
    }

    let contents_dir = bundle_path.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    let resources_dir = contents_dir.join("Resources");
    fs::create_dir_all(&macos_dir)
        .map_err(|err| format!("failed to create {}: {err}", macos_dir.display()))?;
    fs::create_dir_all(&resources_dir)
        .map_err(|err| format!("failed to create {}: {err}", resources_dir.display()))?;

    let bundled_executable = macos_dir.join(EXECUTABLE_NAME);
    fs::copy(binary_path, &bundled_executable).map_err(|err| {
        format!(
            "failed to copy {} to {}: {err}",
            binary_path.display(),
            bundled_executable.display()
        )
    })?;
    set_executable_permissions(&bundled_executable)?;

    let info_plist = render_info_plist(version);
    let info_plist_path = contents_dir.join("Info.plist");
    fs::write(&info_plist_path, info_plist)
        .map_err(|err| format!("failed to write {}: {err}", info_plist_path.display()))?;

    let pkg_info_path = contents_dir.join("PkgInfo");
    fs::write(&pkg_info_path, PKG_INFO)
        .map_err(|err| format!("failed to write {}: {err}", pkg_info_path.display()))?;

    let icns_bytes = build_icns(ICON_SVG)?;
    let icns_path = resources_dir.join(ICON_FILE_NAME);
    fs::write(&icns_path, icns_bytes)
        .map_err(|err| format!("failed to write {}: {err}", icns_path.display()))?;

    Ok(bundle_path)
}

fn render_info_plist(version: &str) -> String {
    INFO_PLIST_TEMPLATE.replace("@VERSION@", version)
}

fn build_icns(svg: &str) -> Result<Vec<u8>, String> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default())
        .map_err(|err| format!("failed to parse icon SVG: {err}"))?;
    let svg_size = tree.size();

    let mut png_by_size: HashMap<u32, Vec<u8>> = HashMap::new();
    for &(_, px_size) in ICNS_SLOTS {
        if png_by_size.contains_key(&px_size) {
            continue;
        }
        let mut pixmap = tiny_skia::Pixmap::new(px_size, px_size)
            .ok_or_else(|| format!("invalid icon size {px_size}x{px_size}"))?;
        let transform = tiny_skia::Transform::from_scale(
            px_size as f32 / svg_size.width(),
            px_size as f32 / svg_size.height(),
        );
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        let png = pixmap
            .encode_png()
            .map_err(|err| format!("failed to encode {px_size}x{px_size} PNG: {err}"))?;
        png_by_size.insert(px_size, png);
    }

    let chunks_len: usize = ICNS_SLOTS
        .iter()
        .map(|(_, px_size)| 8 + png_by_size[px_size].len())
        .sum();
    let total_len = 8 + chunks_len;
    let total_len_u32 =
        u32::try_from(total_len).map_err(|_| "generated .icns exceeds u32 size limit")?;

    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&total_len_u32.to_be_bytes());
    for &(ostype, px_size) in ICNS_SLOTS {
        let png = &png_by_size[&px_size];
        let chunk_len = u32::try_from(8 + png.len())
            .map_err(|_| "generated .icns chunk exceeds u32 size limit")?;
        out.extend_from_slice(&ostype);
        out.extend_from_slice(&chunk_len.to_be_bytes());
        out.extend_from_slice(png);
    }
    Ok(out)
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|err| format!("failed to chmod {}: {err}", path.display()))
}

#[cfg(not(unix))]
fn set_executable_permissions(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn codesign_if_macos(bundle_path: &Path) -> Result<(), String> {
    let status = Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(bundle_path)
        .status()
        .map_err(|err| format!("failed to invoke codesign: {err}"))?;
    if !status.success() {
        return Err(format!("codesign exited with {status}"));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn codesign_if_macos(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
#[path = "../bundle_macos_test.rs"]
mod tests;

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    BundleCli, BundleOptions, ICNS_SLOTS, ICON_SVG, assemble_bundle, build_icns, parse_args,
};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn parse(args: &[&str]) -> Result<BundleCli, String> {
    parse_args(args.iter().map(|arg| (*arg).to_string()))
}

fn temp_test_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "lumi_bundle_macos_{label}_{}_{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

#[test]
fn test_parse_args_supports_profile_target_and_output_flags() {
    assert_eq!(
        parse(&[]),
        Ok(BundleCli::Build(BundleOptions {
            release: true,
            target: None,
            out_dir: None,
        }))
    );
    assert_eq!(
        parse(&[
            "--debug",
            "--target",
            "aarch64-apple-darwin",
            "--out-dir=/Applications"
        ]),
        Ok(BundleCli::Build(BundleOptions {
            release: false,
            target: Some("aarch64-apple-darwin".to_string()),
            out_dir: Some(PathBuf::from("/Applications")),
        }))
    );
    assert_eq!(parse(&["-h"]), Ok(BundleCli::Help));
    assert_eq!(parse(&["--help"]), Ok(BundleCli::Help));
    assert!(parse(&["--out-dir"]).is_err());
    assert!(parse(&["--target="]).is_err());
    assert!(parse(&["--bogus"]).is_err());
}

#[test]
fn test_build_icns_packs_valid_png_chunks_for_all_macos_icon_slots() {
    let icns = build_icns(ICON_SVG).expect("build .icns from icon.svg");
    assert_eq!(&icns[0..4], b"icns");
    let total_len = u32::from_be_bytes(icns[4..8].try_into().expect("4-byte length")) as usize;
    assert_eq!(total_len, icns.len());

    let mut cursor = 8;
    let mut parsed_slots = Vec::new();
    while cursor < icns.len() {
        let ostype: [u8; 4] = icns[cursor..cursor + 4].try_into().expect("4-byte ostype");
        let chunk_len =
            u32::from_be_bytes(icns[cursor + 4..cursor + 8].try_into().expect("chunk len"))
                as usize;
        assert!(chunk_len > 8 + PNG_SIGNATURE.len());
        let payload = &icns[cursor + 8..cursor + chunk_len];
        assert_eq!(&payload[..8], PNG_SIGNATURE);

        // PNG IHDR begins immediately after the 8-byte signature + 4-byte length + "IHDR":
        // bytes 16..20 are big-endian width and 20..24 are big-endian height.
        assert_eq!(&payload[12..16], b"IHDR");
        let width = u32::from_be_bytes(payload[16..20].try_into().expect("width"));
        let height = u32::from_be_bytes(payload[20..24].try_into().expect("height"));
        assert_eq!(width, height);
        parsed_slots.push((ostype, width));
        cursor += chunk_len;
    }
    assert_eq!(cursor, icns.len());
    assert_eq!(parsed_slots, ICNS_SLOTS);
}

#[test]
fn test_assemble_bundle_creates_standard_macos_app_layout() {
    let dir = temp_test_dir("layout");
    let fake_binary = dir.join("lumi-app");
    fs::write(&fake_binary, b"fake-mach-o-binary").expect("write fake binary");

    let out_dir = dir.join("dist");
    let bundle = assemble_bundle(&fake_binary, &out_dir, "0.2.0").expect("assemble bundle");
    assert_eq!(bundle, out_dir.join("Lumi.app"));

    let executable = bundle.join("Contents/MacOS/lumi-app");
    assert_eq!(
        fs::read(&executable).expect("read bundled binary"),
        b"fake-mach-o-binary"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = fs::metadata(&executable)
            .expect("stat executable")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    assert_eq!(
        fs::read(bundle.join("Contents/PkgInfo")).expect("read PkgInfo"),
        b"APPL????"
    );

    let info_plist =
        fs::read_to_string(bundle.join("Contents/Info.plist")).expect("read Info.plist");
    assert!(!info_plist.contains("@VERSION@"));
    assert!(info_plist.contains("<string>0.2.0</string>"));
    assert!(info_plist.contains("<string>io.github.lencerf.lumi</string>"));
    assert!(info_plist.contains("<string>lumi-app</string>"));
    assert!(info_plist.contains("<string>bean</string>"));
    assert!(info_plist.contains("<string>beancount</string>"));

    let icns = fs::read(bundle.join("Contents/Resources/Lumi.icns")).expect("read Lumi.icns");
    assert_eq!(&icns[0..4], b"icns");

    fs::remove_dir_all(&dir).expect("clean up temp dir");
}

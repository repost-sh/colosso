use super::*;
use std::path::PathBuf;
use std::process::Command;

const HEADERS: &str = "156.0.14+g4fa4ab5+chromium-156.0.8078.48";

/// What `script/cef-framework manifest` says of a framework built with `args` (an `args.gn`)
/// from sources carrying the patches `patches`, read as the host reads it.
fn built_with(args: &str, patches: &[&str]) -> Framework {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("args.gn"), args).unwrap();
    let applied = dir.path().join("koyo-patches");
    std::fs::create_dir(&applied).unwrap();
    for name in patches {
        std::fs::write(applied.join(format!("{name}.patch")), format!("{name}\n")).unwrap();
    }
    let script = PathBuf::from(env!("KOYO_CHECKOUT")).join("script/cef-framework");
    let said = Command::new(script)
        .arg("manifest")
        .arg(dir.path().join("args.gn"))
        .arg(&applied)
        .output()
        .unwrap();
    assert!(
        said.status.success(),
        "{}",
        String::from_utf8_lossy(&said.stderr)
    );
    parse(&String::from_utf8(said.stdout).unwrap())
}

#[test]
fn a_framework_script_cef_built_plays_h264_and_names_its_patches() {
    let args = "blink_symbol_level=0\nffmpeg_branding=\"Chrome\"\nis_official_build=true\n\
                proprietary_codecs=true\nsymbol_level=0\n";
    let built = built_with(
        args,
        &["devtools-pipe-embedder", "devtools-pipe-descriptors"],
    );
    let expected = Framework {
        codecs: true,
        patches: vec![
            "devtools-pipe-descriptors".into(),
            "devtools-pipe-embedder".into(),
        ],
    };
    assert_eq!(built, expected);
    assert!(built.is_koyos());
    let spaced = built_with(
        "ffmpeg_branding = \"Chrome\"\nproprietary_codecs = true\n",
        &[],
    );
    assert!(spaced.codecs, "args.gn as `gn args` writes it");
}

#[test]
fn a_framework_built_without_both_codec_switches_plays_no_h264() {
    let branded_only = built_with("ffmpeg_branding=\"Chrome\"\n", &["extension-tab-activate"]);
    assert!(!branded_only.codecs);
    assert!(!branded_only.is_koyos());
    let proprietary_only = built_with("proprietary_codecs = true\n", &[]);
    assert!(!proprietary_only.codecs);
}

#[test]
fn cef_own_framework_says_nothing_and_has_neither() {
    assert_eq!(parse(""), Framework::default());
    assert!(!parse("").is_koyos());
}

#[test]
fn the_cef_the_host_was_built_against_agrees() {
    let commit = "4fa4ab516dacf6c49fc7e6e70d2b7ebce29fa35f";
    assert_eq!(agrees(Some("e01e9208"), Some(commit), HEADERS), Ok(()));
}

#[test]
fn a_framework_of_another_cef_is_refused_and_says_which_it_is() {
    let other = "0123456789abcdef0123456789abcdef01234567";
    let refused = agrees(Some("e01e9208"), Some(other), HEADERS).unwrap_err();
    assert!(refused.contains(other), "{refused}");
    assert!(refused.contains(HEADERS), "{refused}");
    assert!(refused.contains("script/browser-engine"), "{refused}");
    let unknown_api = agrees(None, Some(other), HEADERS).unwrap_err();
    assert!(unknown_api.contains("CEF API"), "{unknown_api}");
    assert!(agrees(Some("e01e9208"), None, HEADERS).is_err());
}

#[test]
fn the_framework_in_place_is_the_cef_the_host_was_built_against() {
    use std::os::unix::ffi::OsStrExt as _;
    let dir = cef::sys::get_cef_dir().expect("cef-dll-sys keeps a CEF distribution");
    let binary = dir.join(cef::sys::FRAMEWORK_PATH);
    let name = std::ffi::CString::new(binary.as_os_str().as_bytes()).unwrap();
    // SAFETY: a path to the framework's binary, zero-terminated, alive through the call.
    assert_eq!(unsafe { cef::load_library(Some(&*name.as_ptr().cast())) }, 1);
    assert_eq!(check(), Ok(()));
}

use std::fs;
use std::path::{Path, PathBuf};

use gpui_kit::WindowOptions;

use super::base;

#[test]
fn a_window_keeps_the_displays_full_rate_in_low_power_mode() {
    let options = base();
    let gpui = WindowOptions::default();
    assert_eq!(
        options.low_power_frame_interval, None,
        "a Koyo window is held to a frame interval in Low Power Mode"
    );
    assert_eq!(
        (options.inactive_frame_interval, options.thermal_frame_interval),
        (gpui.inactive_frame_interval, gpui.thermal_frame_interval),
        "a window behind another app's and one under thermal pressure keep GPUI's caps"
    );
}

/// The crate's Rust sources, but for this module's own.
fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_none_or(|name| name != "window_options") {
                sources(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path.file_name().is_none_or(|name| name != "window_options.rs")
        {
            found.push(path);
        }
    }
}

/// What `text` builds as window options other than from [`base`]: each `WindowOptions { … }`
/// whose `..` isn't `window_options::base()`, and each of GPUI's own defaults, by line.
fn built_otherwise(text: &str) -> Vec<usize> {
    let line_of = |at: usize| text[..at].matches('\n').count() + 1;
    let mut found: Vec<usize> = (text.match_indices("WindowOptions::default()"))
        .map(|(at, _)| line_of(at))
        .collect();
    for (at, literal) in text.match_indices("WindowOptions {") {
        if text[..at].trim_end().ends_with("->") {
            continue;
        }
        let body = &text[at + literal.len()..];
        let mut depth = 1;
        let end = body
            .char_indices()
            .find(|&(_, c)| {
                depth += match c {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                };
                depth == 0
            })
            .map_or(body.len(), |(end, _)| end);
        let from_base = ["..crate::window_options::base()", "..window_options::base()"];
        if !from_base.iter().any(|base| body[..end].contains(base)) {
            found.push(line_of(at));
        }
    }
    found
}

#[test]
fn every_window_koyo_opens_starts_from_its_base_options() {
    let mut files = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let otherwise: Vec<String> = (files.iter())
        .flat_map(|path| {
            let text = fs::read_to_string(path).unwrap();
            let shown = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().display().to_string();
            built_otherwise(&text).into_iter().map(move |line| format!("{shown}:{line}"))
        })
        .collect();
    assert!(
        otherwise.is_empty(),
        "window options built other than from `..crate::window_options::base()`, which keeps a \
         window at the display's rate in Low Power Mode: {otherwise:?}"
    );
}

#[test]
fn the_check_sees_options_built_from_gpuis_default() {
    let from_gpui = "let o = WindowOptions {\n    focus: true,\n    ..Default::default()\n};";
    let plain = "let o = gpui_kit::WindowOptions::default();";
    let ours = "fn f() -> WindowOptions {\n    WindowOptions {\n        bounds: Some(b),\n        ..crate::window_options::base()\n    }\n}";
    assert_eq!(built_otherwise(from_gpui), [1]);
    assert_eq!(built_otherwise(plain), [1]);
    assert_eq!(built_otherwise(ours), Vec::<usize>::new());
}

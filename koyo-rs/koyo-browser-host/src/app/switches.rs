//! The switches the browser process starts with: Chrome's own windows and prompts turned off,
//! a page hidden only by the app, and the work Chrome does for its metrics turned off, which a
//! build of Koyo's never sends.

use cef::{CefString, CommandLine, ImplCommandLine};

/// Chrome's own windows, prompts and background life, which a page of Koyo's has no use for.
const CHROME_UI: [&str; 7] = [
    "disable-background-mode",
    "no-default-browser-check",
    "no-first-run",
    "disable-chrome-login-prompt",
    "no-startup-window",
    "disable-session-crashed-bubble",
    "hide-crash-restore-bubble",
];

/// Chromium never takes a page to be hidden by what covers its window. The app hides a page
/// it doesn't show by taking its views out of every window (`native.rs`), which Chromium does
/// take as hidden: no frames, its timers a tick a second (`tests/smoke/sight.rs`). By what
/// covers it, a page an agent uses out of sight, parked in its holder past the screen's corner,
/// went hidden whenever Koyo was behind another app or the screen was locked, and stopped
/// getting frames (`docs/native-pages.md` 5). CEF's own `WasHidden` does nothing for a
/// Chrome-style page, so the views are how.
const COVERED_IS_SHOWN: &str = "disable-backgrounding-occluded-windows";

/// Chrome's sampling of its threads' stacks for its metrics. A Chromium-branded build has no
/// release channel, so Chrome takes it for a developer's and samples on every launch: the
/// browser's main and IO threads for 30 s at 10 a second after it starts and again every
/// 25 minutes or so, and it has the GPU process, the network service and one renderer in five
/// sample themselves (`--start-stack-profiler`). Nothing reads the samples here.
const NO_STACK_SAMPLING: &str = "disable-stack-profiler";

/// Chrome's features turned off: allocation sampling for its metrics, in one launch in a
/// hundred, which nothing reads here either.
const FEATURES_OFF: [&str; 1] = ["HeapProfilerReporting"];

/// Adds the browser process's switches to `command_line`.
pub(super) fn browser(command_line: &mut CommandLine) {
    for switch in CHROME_UI.into_iter().chain([COVERED_IS_SHOWN, NO_STACK_SAMPLING]) {
        command_line.append_switch(Some(&switch.into()));
    }
    let name = CefString::from("disable-features");
    let had = CefString::from(&command_line.switch_value(Some(&name))).to_string();
    let features = with_features(&had, &FEATURES_OFF);
    command_line.append_switch_with_value(Some(&name), Some(&features.as_str().into()));
}

/// A `--disable-features` list of `had`'s and `more`'s, each once. CEF writes its own before
/// the app's switches are added, and a switch given again replaces what it said: Chromium's
/// features CEF turns off stay off.
fn with_features(had: &str, more: &[&str]) -> String {
    let mut features: Vec<&str> = had.split(',').filter(|feature| !feature.is_empty()).collect();
    for feature in more {
        if !features.contains(feature) {
            features.push(feature);
        }
    }
    features.join(",")
}

#[cfg(test)]
mod tests {
    use super::with_features;

    #[test]
    fn the_features_cef_turns_off_stay_off_beside_the_apps() {
        let cef = "GlicActorUi,AutofillActorMode,LensOverlay";
        assert_eq!(
            with_features(cef, &["HeapProfilerReporting"]),
            "GlicActorUi,AutofillActorMode,LensOverlay,HeapProfilerReporting"
        );
        assert_eq!(with_features("", &["A", "B"]), "A,B");
        assert_eq!(with_features("A,B", &["B"]), "A,B", "each once");
    }
}

//! The switches the browser process starts with: Chrome's own windows and prompts turned off,
//! a page hidden only by the app, and its timers run at least once a second while hidden, the
//! work Chrome does for its metrics turned off, which a build of Koyo's never sends, and what
//! Chrome fetches in the background for its omnibox, its AI and translate features and
//! autofill's server, which no page of Koyo's shows, and the copy of its own bundle Chrome
//! makes at each start.

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

/// Chrome's throttling of a silent hidden page's timers to once a minute, a minute after it
/// was hidden (five if it hadn't painted yet; `third_party/blink/common/features.cc`). A page
/// in another space or tab is hidden, and a player there that went silent between songs or
/// while it buffers must still fetch and retry: its timers run once a second, as every hidden
/// page's do (`tests/smoke/sight.rs`).
const WAKE_UPS_EACH_SECOND: [&str; 1] = ["IntensiveWakeUpThrottling"];

/// Chrome's sampling of its threads' stacks for its metrics. A Chromium-branded build has no
/// release channel, so Chrome takes it for a developer's and samples on every launch: the
/// browser's main and IO threads for 30 s at 10 a second after it starts and again every
/// 25 minutes or so, and it has the GPU process, the network service and one renderer in five
/// sample themselves (`--start-stack-profiler`). Nothing reads the samples here.
const NO_STACK_SAMPLING: &str = "disable-stack-profiler";

/// Chrome's features turned off: allocation sampling for its metrics, in one launch in a
/// hundred, which nothing reads here either.
const METRICS_OFF: [&str; 1] = ["HeapProfilerReporting"];

/// Chrome's omnibox and the search behind it, which Koyo's address field is instead of: the
/// model the omnibox suggests from offline, a component a fresh profile fetched in its first
/// minutes; the check of whether Google offers its AI Mode there, a request to
/// www.google.com with its cookies as every launch starts; and the connection to the search
/// engine Chrome keeps warm, opened five seconds after the start and again each minute.
const OMNIBOX_OFF: [&str; 4] = [
    "OmniboxOnDeviceHeadProviderNonIncognito",
    "OmniboxOnDeviceHeadProviderIncognito",
    "AimEnabled",
    "PreconnectToSearch",
];

/// What Google's optimization guide sends Chrome: hints for its own features, a component
/// fetched for none Koyo shows, and the on-device models Chrome's AI features run.
const GUIDE_OFF: [&str; 2] = ["OptimizationHints", "OptimizationGuideModelExecution"];

/// The model that decides whether to offer translating a page, fetched as the first page
/// loads, for a translate bubble Koyo never shows.
const TRANSLATE_OFF: [&str; 2] = ["TranslateRankerQuery", "TranslateRankerEnforcement"];

/// Autofill's questions to Google about a form's fields, which need an API key Koyo's build
/// hasn't and are refused. What a person saved fills in all the same.
const AUTOFILL_SERVER_OFF: [&str; 1] = ["AutofillServerCommunication"];

/// The clone of its own bundle Chrome makes in the user's temporary folder at each start
/// (`chrome/browser/mac/code_sign_clone_manager.mm`), so that an update replacing the bundle
/// leaves the running browser's signature valid. The copies a host didn't delete piled up:
/// 150 of 400 MB in three days, 13 GB on disk once their shared blocks are counted once.
const BUNDLE_CLONE_OFF: [&str; 1] = ["MacAppCodeSignClone"];

/// Chromium's separate key for a profile outside Chrome's own data directory
/// (`browser_process_impl.cc`, since Chromium 158): it marks the key the profiles' cookies and
/// passwords were saved under, "v10", unavailable and saves new ones under a key of its own,
/// so Chromium deletes every cookie saved before, as it can't decrypt them. It keeps a second
/// data directory of Chrome's from reading the first one's secrets with the same key. Koyo's
/// engine has one data directory, its root (`host/start.rs`), and a Keychain item of its own
/// ("Koyo Browser Engine"): that root is its default, and what it saved stays readable.
const ONE_DATA_DIRECTORY: [&str; 1] = ["KeychainKeyProviderUserDataDirProtection"];

/// Every feature the browser process turns off. What stays on, and why, is in the tests:
/// the components and requests that check certificates, warn of dangerous sites and
/// downloads, play protected video and keep extensions current.
fn features_off() -> impl Iterator<Item = &'static str> {
    METRICS_OFF
        .into_iter()
        .chain(WAKE_UPS_EACH_SECOND)
        .chain(OMNIBOX_OFF)
        .chain(GUIDE_OFF)
        .chain(TRANSLATE_OFF)
        .chain(AUTOFILL_SERVER_OFF)
        .chain(BUNDLE_CLONE_OFF)
        .chain(ONE_DATA_DIRECTORY)
}

/// Every switch the browser process starts with.
fn switches() -> impl Iterator<Item = &'static str> {
    CHROME_UI
        .into_iter()
        .chain([COVERED_IS_SHOWN, NO_STACK_SAMPLING])
}

/// Adds the browser process's switches to `command_line`.
pub(super) fn browser(command_line: &mut CommandLine) {
    for switch in switches() {
        command_line.append_switch(Some(&switch.into()));
    }
    let name = CefString::from("disable-features");
    let had = CefString::from(&command_line.switch_value(Some(&name))).to_string();
    let off: Vec<&str> = features_off().collect();
    let features = with_features(&had, &off);
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
    use super::{features_off, switches, with_features};

    /// What a fresh profile fetched in its first minutes, and every launch asked Google, for
    /// nothing a page of Koyo's shows: each stays off.
    const OFF: [&str; 11] = [
        "HeapProfilerReporting",
        "OmniboxOnDeviceHeadProviderNonIncognito",
        "OmniboxOnDeviceHeadProviderIncognito",
        "AimEnabled",
        "PreconnectToSearch",
        "OptimizationHints",
        "OptimizationGuideModelExecution",
        "TranslateRankerQuery",
        "TranslateRankerEnforcement",
        "AutofillServerCommunication",
        "MacAppCodeSignClone",
    ];

    /// Switches that would stop what a page needs along with what it doesn't: background
    /// networking takes extensions' updates and Safe Browsing's lists with it, and component
    /// updates Widevine's video decryption, the certificate revocation list and the
    /// certificate transparency logs.
    const NEVER_SWITCHED: [&str; 2] = ["disable-background-networking", "disable-component-update"];

    /// Features that serve the person, or whose being off does the opposite of what it
    /// sounds like: without attestations every site may use the Privacy Sandbox's APIs; the
    /// subresource filter marks heavy ads; the media engagement list lets the sites a person
    /// plays video on autoplay with sound; network time tells a certificate error from a
    /// wrong clock.
    const NEVER_OFF: [&str; 4] = [
        "EnforcePrivacySandboxAttestations",
        "SubresourceFilter",
        "PreloadMediaEngagementData",
        "NetworkTimeServiceQuerying",
    ];

    #[test]
    fn what_serves_no_page_of_koyos_is_off_and_what_keeps_pages_safe_stays_on() {
        let off: Vec<&str> = features_off().collect();
        for feature in OFF {
            assert!(off.contains(&feature), "{feature} is off");
        }
        for feature in NEVER_OFF {
            assert!(!off.contains(&feature), "{feature} stays on");
        }
        let given: Vec<&str> = switches().collect();
        for switch in NEVER_SWITCHED {
            assert!(!given.contains(&switch), "no --{switch}");
        }
    }

    /// Chromium 158 deleted every cookie the profiles had saved, as Koyo's root isn't Chrome's
    /// own data directory: the key they were saved under stays theirs.
    #[test]
    fn what_a_profile_saved_stays_readable_under_its_key() {
        let off: Vec<&str> = features_off().collect();
        assert!(off.contains(&"KeychainKeyProviderUserDataDirProtection"));
    }

    /// What Chrome does with a hidden page that Koyo keeps: its timers at once a second, and its
    /// renderer at the priority of a page in the background.
    const HIDDEN_RESTS: [&str; 2] = ["disable-background-timer-throttling", "disable-renderer-backgrounding"];

    #[test]
    fn a_hidden_pages_timers_run_each_second_not_each_minute() {
        let off: Vec<&str> = features_off().collect();
        assert!(off.contains(&"IntensiveWakeUpThrottling"), "no timers once a minute");
        let given: Vec<&str> = switches().collect();
        for switch in HIDDEN_RESTS {
            assert!(!given.contains(&switch), "no --{switch}");
        }
    }

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

//! The profiles' folder while no engine holds it: browsing data cleared then is gone before
//! the engine starts, and what the engine writes as it starts stays; a deleted profile's
//! folders go as the app launches, with no page asked for.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use gpui_kit::TestAppContext;
use koyo_browser_ipc::client::Launch;

use super::{Browsers, TestLaunch};
use crate::test_support::scratch;

/// A stand-in for the engine in `scratch`: as it starts, it writes a file into the profiles'
/// folder, as Chromium does, then goes without saying hello.
fn engine(scratch: &Path) -> PathBuf {
    let host = scratch.join("host");
    let script = "#!/bin/sh\nmkdir -p \"$KOYO_BROWSER_PROFILES\"\n\
                  echo started > \"$KOYO_BROWSER_PROFILES/started\"\n";
    std::fs::write(&host, script).unwrap();
    std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o755)).unwrap();
    host
}

#[gpui_kit::test(iterations = 10)]
fn browsing_data_cleared_with_no_engine_is_gone_before_it_starts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let scratch = scratch("browser-clear-before-start");
    let profiles = scratch.join("profiles");
    let cookies = profiles.join("profile-default").join("Cookies");
    std::fs::create_dir_all(cookies.parent().unwrap()).unwrap();
    std::fs::write(&cookies, "a sign-in").unwrap();
    let launch = Launch {
        executable: engine(&scratch),
        profiles: profiles.clone(),
        passkeys: false,
    };
    cx.update(|cx| {
        crate::test_support::init(cx);
        cx.set_global(TestLaunch(launch));
        Browsers::global(cx).update(cx, |browsers, cx| {
            browsers.clear_data(cx);
            browsers.start_if_idle("a test", cx);
        });
    });
    cx.run_until_parked();

    assert!(!cookies.exists(), "the sign-in is still on disk");
    let started = std::fs::read_to_string(profiles.join("started")).map_err(|error| error.kind());
    assert_eq!(started, Ok("started\n".into()), "what the engine wrote as it started");
}

#[gpui_kit::test]
fn a_deleted_profiles_folders_go_as_the_app_launches(cx: &mut TestAppContext) {
    let scratch = scratch("browser-deleted-at-launch");
    let profiles = scratch.join("profiles");
    for folder in ["profile-work-1", "profile-home-2"] {
        std::fs::create_dir_all(profiles.join(folder)).unwrap();
    }
    koyo_browser_host::forget_profile(&profiles, "work-1").unwrap();
    let launch = Launch {
        executable: scratch.join("no-engine"),
        profiles: profiles.clone(),
        passkeys: false,
    };
    cx.update(|cx| {
        cx.set_global(TestLaunch(launch));
        crate::test_support::init(cx);
    });
    cx.run_until_parked();

    assert!(!profiles.join("profile-work-1").exists(), "the deleted profile's folder");
    assert!(profiles.join("profile-home-2").exists(), "a kept profile's folder went");
    cx.update(|cx| assert!(crate::browser::engine_idle(cx), "an engine started for it"));
}

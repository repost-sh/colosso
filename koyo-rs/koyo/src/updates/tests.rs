use std::path::PathBuf;

use futures::FutureExt as _;
use gpui_kit::TestAppContext;

use super::fixtures::{self, Koyo, koyo, recorded, says, until_it_says};
use super::installed::Unreleased;
use super::{TheUpdates, act, hand_over_to_reopen};

#[gpui_kit::test]
fn a_release_out_at_launch_is_offered(cx: &mut TestAppContext) {
    let koyo = koyo("updates-offered", true, cx);
    assert_eq!(says(cx), ("quiet", "Checking for updates…".into()));
    koyo.announce("9.0.0");
    cx.run_until_parked();
    assert_eq!(
        says(cx),
        (
            "Update Koyo",
            "Koyo 9.0.0 is available. Koyo downloads it, then restarts to install it.".into()
        )
    );
    assert_eq!(koyo.restarts.get(), 0);
}

#[gpui_kit::test]
fn the_feed_is_read_at_launch_and_again_every_hour(cx: &mut TestAppContext) {
    let koyo = koyo("updates-hourly", true, cx);
    cx.run_until_parked();
    let (label, said) = says(cx);
    assert_eq!(label, "quiet");
    assert!(said.starts_with("Couldn't check for updates: 404"), "{said}");
    assert!(said.ends_with(" Koyo tries again within the hour."), "{said}");

    koyo.announce(env!("CARGO_PKG_VERSION"));
    cx.run_until_parked();
    assert!(says(cx).1.starts_with("Couldn't check"), "the feed was read before the hour was up");
    Koyo::an_hour_on(cx);
    assert_eq!(says(cx), ("quiet", "Koyo is up to date. Checked just now.".into()));

    koyo.announce("9.0.0");
    Koyo::an_hour_on(cx);
    assert_eq!(says(cx).0, "Update Koyo");

    koyo.announce(env!("CARGO_PKG_VERSION"));
    Koyo::an_hour_on(cx);
    assert_eq!(says(cx).0, "quiet", "a release withdrawn is still offered");
}

#[gpui_kit::test]
fn one_click_installs_and_asks_for_the_restart_once(cx: &mut TestAppContext) {
    let koyo = koyo("updates-click", true, cx);
    koyo.publish(cx);

    cx.update(act);
    assert_eq!(says(cx), ("working", "Downloading Koyo 9.0.0: 0 of 1 MB.".into()));
    cx.update(act);
    until_it_says("Restart to Update", cx);

    assert_eq!(koyo.restarts.get(), 1, "the click that said update ends in one restart");
    assert_eq!(
        says(cx).1,
        "Koyo 9.0.0 is ready. It installs when Koyo restarts or quits."
    );
    let attempts = fixtures::attempts(&koyo.bundle);
    assert_eq!(attempts.len(), 1, "a second click started a second install");
    assert!(attempts[0].join("Koyo.app/Contents/MacOS/koyo").is_file());

    koyo.announce("9.1.0");
    Koyo::an_hour_on(cx);
    assert_eq!(says(cx).1, "Koyo 9.0.0 is ready. It installs when Koyo restarts or quits.");

    cx.update(act);
    cx.run_until_parked();
    assert_eq!(koyo.restarts.get(), 2, "a click on Restart to Update asks for the restart");
}

#[gpui_kit::test]
fn one_click_ends_in_a_restart_through_the_quit_path(cx: &mut TestAppContext) {
    cx.update(crate::quitting::init);
    let koyo = koyo("updates-through-quit", true, cx);
    koyo.publish(cx);

    cx.update(act);
    let told = koyo.handed_over(cx);

    let attempt = fixtures::attempts(&koyo.bundle).remove(0);
    assert_eq!(told, recorded(&attempt, true));
    assert_eq!(says(cx), ("working", "Restarting into Koyo 9.0.0.".into()));
    assert_eq!(koyo.restarts.get(), 1);
}

#[gpui_kit::test]
fn a_staged_release_is_handed_over_at_a_plain_quit_and_nothing_is_reopened(cx: &mut TestAppContext) {
    let koyo = koyo("updates-quit", true, cx);
    koyo.stage(cx);
    let attempt = fixtures::attempts(&koyo.bundle).remove(0);

    koyo.quit(cx);

    assert_eq!(koyo.told(), recorded(&attempt, false));
    assert!(attempt.is_dir());
}

#[gpui_kit::test]
fn a_restart_hands_the_release_over_to_be_reopened(cx: &mut TestAppContext) {
    let koyo = koyo("updates-restart", true, cx);
    koyo.stage(cx);
    let attempt = fixtures::attempts(&koyo.bundle).remove(0);

    let handed = cx.update(hand_over_to_reopen);
    assert_eq!(says(cx), ("working", "Restarting into Koyo 9.0.0.".into()));
    cx.run_until_parked();

    assert_eq!(handed.now_or_never(), Some(true));
    assert_eq!(koyo.told(), recorded(&attempt, true));

    let again = cx.update(hand_over_to_reopen);
    cx.run_until_parked();
    assert_eq!(again.now_or_never(), Some(false), "a release is handed over once");
    std::fs::remove_file(&koyo.told).unwrap();
    koyo.quit(cx);
    assert_eq!(says(cx).0, "working");
    Koyo::an_hour_on(cx);
    assert_eq!(says(cx).1, "Restarting into Koyo 9.0.0.");
    assert!(!koyo.told.exists(), "the quit after a hand-over started a second installer");
}

#[gpui_kit::test]
fn an_installer_that_does_not_start_leaves_koyo_open_and_the_update_failed(cx: &mut TestAppContext) {
    let koyo = koyo("updates-no-installer", false, cx);
    koyo.stage(cx);

    let handed = cx.update(hand_over_to_reopen);
    cx.run_until_parked();

    assert_eq!(handed.now_or_never(), Some(false), "the app would have quit with no installer");
    let (label, said) = says(cx);
    assert_eq!(label, "Retry Update");
    let began = "Couldn't update to Koyo 9.0.0. Koyo couldn't start its installer: ";
    assert!(said.starts_with(began), "{said}");
    assert_eq!(fixtures::attempts(&koyo.bundle), Vec::<PathBuf>::new());
}

#[gpui_kit::test]
fn a_failed_install_offers_to_try_again_and_the_retry_installs(cx: &mut TestAppContext) {
    let koyo = koyo("updates-retry", true, cx);
    let release = koyo.publish(cx);
    let image = koyo.bucket.join("Koyo-9.0.0.dmg");
    let kept = koyo.bucket.join("kept.dmg");
    std::fs::rename(&image, &kept).unwrap();

    cx.update(act);
    until_it_says("Retry Update", cx);

    let said = says(cx).1;
    let began = "Couldn't update to Koyo 9.0.0. The download didn't arrive: 404";
    assert!(said.starts_with(began), "{said}");
    assert_eq!(koyo.restarts.get(), 0);
    assert_eq!(fixtures::attempts(&koyo.bundle), Vec::<PathBuf>::new());
    Koyo::an_hour_on(cx);
    assert_eq!(says(cx).0, "Retry Update", "a check let go of a failure for the same release");

    std::fs::rename(&kept, &image).unwrap();
    assert_eq!(std::fs::metadata(&image).unwrap().len(), release.bytes);
    cx.update(act);
    until_it_says("Restart to Update", cx);
    assert_eq!(koyo.restarts.get(), 1);
}

#[gpui_kit::test]
fn nothing_is_handed_over_while_nothing_is_staged(cx: &mut TestAppContext) {
    let koyo = koyo("updates-nothing-staged", true, cx);
    koyo.announce("9.0.0");
    cx.run_until_parked();

    let handed = cx.update(hand_over_to_reopen);
    cx.run_until_parked();
    assert_eq!(handed.now_or_never(), Some(false));
    koyo.quit(cx);
    assert_eq!(says(cx).0, "Update Koyo");
    assert!(!koyo.told.exists());
}

#[gpui_kit::test]
fn a_build_that_was_not_released_offers_nothing_and_says_why(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(TheUpdates::Unreleased(Unreleased::NoFeed)));
    cx.update(act);
    let handed = cx.update(hand_over_to_reopen);
    assert_eq!(handed.now_or_never(), Some(false));
    assert_eq!(
        says(cx),
        (
            "quiet",
            "This build doesn't update itself: it was built here, not released.".into()
        )
    );
    assert!(cx.read(super::observed).is_none());
}

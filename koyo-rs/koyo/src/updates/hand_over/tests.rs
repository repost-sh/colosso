use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{HandOver, INSTALL_UPDATE, Itself, exchange, hand_over, install};
use crate::blocking::in_test;
use crate::test_support::{scratch, write};
use crate::updates::attempt::{Attempt, sweep};
use crate::updates::fixtures::{self, Made, version_of};
use crate::updates::version::Version;

fn told(reopen: bool) -> HandOver {
    HandOver {
        attempt: "/Applications/.Koyo.app.update-41-7".into(),
        reopen,
    }
}

/// What the installer reads of the arguments `told` is written as.
fn read_back(told: &HandOver) -> Option<HandOver> {
    let mut args = told.args().into_iter().map(|arg| arg.into_string().unwrap());
    assert_eq!(args.next().as_deref(), Some(INSTALL_UPDATE), "the mode's word comes first");
    HandOver::parse(args)
}

#[test]
fn what_the_app_tells_the_installer_reads_back_the_same() {
    for reopen in [false, true] {
        assert_eq!(read_back(&told(reopen)), Some(told(reopen)));
    }
    let spaced = HandOver {
        attempt: "/Users/a person/Apps/.Koyo 2.app.update-41-7".into(),
        reopen: true,
    };
    assert_eq!(read_back(&spaced), Some(spaced));
}

#[test]
fn arguments_that_are_not_a_hand_over_are_refused() {
    let parse = |args: &[&str]| HandOver::parse(args.iter().map(|arg| (*arg).to_owned()));
    assert_eq!(parse(&[]), None);
    assert_eq!(parse(&["--reopen"]), None);
    assert_eq!(parse(&["--reopen", "--attempt"]), None);
    assert_eq!(parse(&["--attempt", "/a", "--pid", "4"]), None);
    assert_eq!(
        parse(&["--attempt", "/a", "--installed", "/b"]),
        None,
        "the installer was told which bundle to replace"
    );
    assert!(parse(&["--reopen", "--attempt", "/a"]).is_some());
}

#[test]
fn two_bundles_are_exchanged_in_one_step() {
    let root = scratch("update-exchange");
    let (staged, installed) = (root.join("staged/Koyo.app"), root.join("Koyo.app"));
    fixtures::bundle(&staged, &Made { identifier: None, ..Made::release("9.0.0") });
    fixtures::bundle(&installed, &Made { identifier: None, ..Made::release("8.0.0") });

    in_test(|may| exchange(&staged, &installed, may)).unwrap();

    assert_eq!(version_of(&installed), "9.0.0");
    assert_eq!(version_of(&staged), "8.0.0");
    let missing = in_test(|may| exchange(&root.join("nothing"), &installed, may));
    assert_eq!(missing.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    assert_eq!(version_of(&installed), "9.0.0", "a failed exchange changed the bundle");
}

/// An installed Koyo, unsigned since the installer checks only what's staged, that says
/// 8.0.0 of itself so a test can tell it from what replaces it: at `<root>/Koyo.app`.
fn installed_at(root: &Path) -> PathBuf {
    let installed = root.join("Koyo.app");
    fixtures::bundle(&installed, &Made { identifier: None, ..Made::release("8.0.0") });
    installed
}

/// A Koyo at `<root>/Koyo.app` (`installed_at`), with a bundle made as `staged` says beside
/// it in an attempt this test holds: the bundle, and the attempt.
fn staged_beside(root: &Path, staged: &Made) -> (PathBuf, Attempt) {
    let installed = installed_at(root);
    let attempt = in_test(|may| Attempt::begin(&installed, may)).unwrap();
    fixtures::bundle(&attempt.staged(), staged);
    (installed, attempt)
}

/// Runs the installer of the Koyo at `bundle` on the attempt `folder`, on a thread of its own
/// as a process of its own would run, trusting what the tests' releases are signed as: the
/// release it put in place, or why not.
fn installing(folder: &Path, bundle: &Path) -> std::thread::JoinHandle<Result<Version, String>> {
    let folder = folder.to_owned();
    let itself = Itself { bundle: bundle.to_owned(), trust: fixtures::trust() };
    std::thread::spawn(move || in_test(|may| install(&folder, &itself, may)))
}

/// Restarts into the bundle made as `staged` says, its attempt let go as the Koyo that held
/// it ends: the installer must refuse it, leave the installed bundle as it was, and remove the
/// attempt.
fn refuses(name: &str, staged: &Made, change: impl FnOnce(&Path)) {
    let root = scratch(name);
    let (installed, attempt) = staged_beside(&root, staged);
    change(&attempt.staged());
    let folder = attempt.folder().to_owned();
    drop(attempt);

    let ended = installing(&folder, &installed).join().unwrap();

    assert!(ended.is_err(), "{name}: the staged bundle was taken: {ended:?}");
    assert_eq!(version_of(&installed), "8.0.0", "{name}: the bundle was replaced");
    assert!(!folder.exists(), "{name}: the refused attempt stayed");
}

#[test]
fn the_installer_waits_for_the_attempt_to_be_let_go_then_replaces_the_bundle() {
    let root = scratch("update-installer");
    let (installed, attempt) = staged_beside(&root, &Made::release("9.0.0"));
    let folder = attempt.folder().to_owned();

    let at_work = installing(&folder, &installed);
    #[expect(
        clippy::disallowed_methods,
        reason = "the installer is given time to do what it must not do yet"
    )]
    std::thread::sleep(Duration::from_millis(300));
    assert!(!at_work.is_finished(), "the installer didn't wait for the attempt's holder");
    assert_eq!(version_of(&installed), "8.0.0", "the bundle was replaced under a living Koyo");

    drop(attempt);
    let ended = at_work.join().unwrap();

    assert_eq!(ended, Ok("9.0.0".parse().unwrap()));
    assert_eq!(version_of(&installed), "9.0.0");
    assert!(!folder.exists(), "the attempt, and the replaced bundle in it, stayed");
    let left: Vec<_> = std::fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().path()).collect();
    assert_eq!(left, [installed]);
}

#[test]
fn a_staged_bundle_that_is_not_signed_as_the_installer_is_refused() {
    let other = Made { identifier: Some("codes.koyo.other"), ..Made::release("9.0.0") };
    refuses("update-installer-other", &other, |_| {});
    let unsigned = Made { identifier: None, ..Made::release("9.0.0") };
    refuses("update-installer-unsigned", &unsigned, |_| {});
    refuses("update-installer-edited", &Made::release("9.0.0"), |staged| {
        let plist = fixtures::info_plist(&Made::release("9.0.1"));
        write(staged, "Contents/Info.plist", plist.as_bytes());
    });
}

#[test]
fn a_staged_bundle_that_is_not_newer_than_the_installer_is_refused() {
    refuses("update-installer-same", &Made::release(env!("CARGO_PKG_VERSION")), |_| {});
    refuses("update-installer-older", &Made::release("0.0.1"), |_| {});
}

#[test]
fn an_attempt_with_nothing_staged_leaves_the_bundle_as_it_was() {
    refuses("update-installer-empty", &Made::release("9.0.0"), |staged| {
        std::fs::remove_dir_all(staged).unwrap();
    });
}

#[test]
fn a_folder_that_is_not_an_attempt_beside_the_installer_s_bundle_is_refused_and_left() {
    let root = scratch("update-installer-elsewhere");
    let installed = installed_at(&root);
    let elsewhere = root.join("elsewhere/.Koyo.app.update-1-1");
    let unnamed = root.join("staging");
    let another_s = root.join(".Other.app.update-1-1");
    for folder in [&elsewhere, &unnamed, &another_s] {
        fixtures::bundle(&folder.join("Koyo.app"), &Made::release("9.0.0"));

        let ended = installing(folder, &installed).join().unwrap();

        let folder = folder.display();
        assert!(ended.is_err(), "{folder} was taken: {ended:?}");
        assert_eq!(version_of(&installed), "8.0.0", "{folder} replaced the bundle");
    }
    for folder in [elsewhere, unnamed, another_s] {
        assert_eq!(version_of(&folder.join("Koyo.app")), "9.0.0", "what isn't the installer's went");
    }
}

#[test]
fn an_attempt_that_cannot_be_taken_leaves_the_bundle_as_it_was() {
    let root = scratch("update-installer-gone");
    let installed = installed_at(&root);
    let folder = root.join(".Koyo.app.update-1-1");

    let ended = installing(&folder, &installed).join().unwrap();

    assert!(ended.is_err(), "{ended:?}");
    assert_eq!(version_of(&installed), "8.0.0");
}

#[test]
fn a_staged_release_is_handed_to_the_installer_with_where_it_is_and_stays_held() {
    let root = scratch("update-hand-over");
    let installed = root.join("Koyo.app");
    let (program, told) = fixtures::recorder(&root);
    for reopen in [false, true] {
        let _ = std::fs::remove_file(&told);
        let staged = fixtures::staged(&installed, "9.0.0");
        let attempt = staged.attempt().to_owned();

        in_test(|may| hand_over(staged, &program, reopen, may)).unwrap();

        fixtures::until("the installer to start", || told.exists());
        assert_eq!(std::fs::read_to_string(&told).unwrap(), fixtures::recorded(&attempt, reopen));

        let taken = File::open(&attempt).unwrap().try_lock();
        assert!(
            matches!(taken, Err(std::fs::TryLockError::WouldBlock)),
            "a release handed over let go of its attempt: {taken:?}"
        );
        in_test(|may| sweep(&installed, may));
        assert!(attempt.is_dir(), "a sweep removed an attempt handed to the installer");
    }
}

#[test]
fn an_installer_that_does_not_start_leaves_nothing_staged_and_says_why() {
    let root = scratch("update-hand-over-failed");
    let installed = root.join("Koyo.app");
    let staged = fixtures::staged(&installed, "9.0.0");
    let attempt = staged.attempt().to_owned();

    let handed = in_test(|may| hand_over(staged, &root.join("no-such-koyo"), true, may));

    let (release, why) = handed.unwrap_err();
    assert_eq!(release, fixtures::release("9.0.0", 1));
    assert!(why.starts_with("Koyo couldn't start its installer: "), "{why}");
    assert!(!attempt.exists());
}

use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::{Halt, Progress, Work, copy_in, run};
use crate::blocking::in_test;
use crate::test_support::scratch;
use crate::updates::attempt::{Attempt, sweep};
use crate::updates::feed::{Feed, Release};
use crate::updates::fixtures::{self, FEED, Folder, IDENTIFIER, Made};
use crate::updates::installed::info;
use crate::updates::stage::Failure;
use crate::updates::staged::Staged;

/// A Koyo in a folder of its own, and the bucket its releases are in.
struct Place {
    bundle: PathBuf,
    bucket: PathBuf,
}

impl Place {
    fn new(name: &str) -> Place {
        let root = scratch(name);
        let (beside, bucket) = (root.join("Applications"), root.join("bucket"));
        std::fs::create_dir_all(&beside).unwrap();
        std::fs::create_dir_all(&bucket).unwrap();
        Place {
            bundle: beside.join("Koyo.app"),
            bucket,
        }
    }

    /// Puts an image of 9.0.0 in the bucket whose app is made as `made` says, signed under
    /// `signed` if it's signed: the release a feed would say it is.
    fn holds(&self, made: &Made, signed: Option<&str>) -> Release {
        let image = self.image();
        fixtures::image(&self.bucket, &image, made, signed);
        fixtures::release("9.0.0", std::fs::metadata(image).unwrap().len())
    }

    fn image(&self) -> PathBuf {
        self.bucket.join("Koyo-9.0.0.dmg")
    }

    fn work(&self, release: Release, fetch: Folder) -> Work {
        Work {
            image: Feed::at(FEED).unwrap().image(release.version),
            release,
            bundle: self.bundle.clone(),
            trust: fixtures::trust(),
            macos: "14.0".parse().unwrap(),
            fetch: Arc::new(fetch),
        }
    }

    /// Installs `release` from the bucket: how it ended, and what the job said on the way.
    fn installs(&self, release: Release) -> (Result<Staged, Halt>, Vec<Progress>) {
        let (_stop, stopped) = channel();
        self.runs(&self.work(release, Folder::at(&self.bucket)), &stopped)
    }

    fn runs(&self, work: &Work, stopped: &Receiver<()>) -> (Result<Staged, Halt>, Vec<Progress>) {
        let said = RefCell::new(Vec::new());
        let ended = in_test(|may| run(work, &|progress| said.borrow_mut().push(progress), stopped, may));
        (ended, said.into_inner())
    }

    /// What an install refused for, having left nothing beside the bundle.
    fn refused(&self, release: Release) -> Failure {
        let (ended, _) = self.installs(release);
        assert_eq!(fixtures::attempts(&self.bundle), Vec::<PathBuf>::new());
        match ended {
            Err(Halt::Failed(failure)) => failure,
            other => panic!("the install wasn't refused: {other:?}"),
        }
    }
}

fn version_of(bundle: &Path) -> String {
    let info = in_test(|may| info(bundle, may)).unwrap();
    info["CFBundleShortVersionString"].as_string().unwrap().to_owned()
}

#[test]
fn a_release_is_downloaded_checked_and_staged_beside_the_bundle() {
    let place = Place::new("update-install");
    let release = place.holds(&Made::release("9.0.0"), Some(IDENTIFIER));
    let bytes = release.bytes;

    let (ended, said) = place.installs(release.clone());

    let staged = ended.unwrap();
    assert_eq!(staged.release(), &release);
    assert_eq!(fixtures::attempts(&place.bundle), [staged.attempt().to_owned()]);
    assert_eq!(version_of(&staged.attempt().join("Koyo.app")), "9.0.0");
    let left: Vec<_> = std::fs::read_dir(staged.attempt())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["Koyo.app"], "the image and its mount point are gone");

    let received: Vec<u64> = said
        .iter()
        .map_while(|progress| match progress {
            Progress::Received(received) => Some(*received),
            _ => None,
        })
        .collect();
    assert!(received.is_sorted() && received.last() == Some(&bytes), "{received:?}");
    assert!(matches!(said[received.len()..], [Progress::Preparing]), "{said:?}");
}

#[test]
fn an_image_that_is_not_signed_is_refused_before_it_is_opened() {
    let place = Place::new("update-install-unsigned");
    let release = place.holds(&Made::release("9.0.0"), None);
    let work = place.work(release.clone(), Folder::at(&place.bucket));
    let (_stop, stopped): (Sender<()>, _) = channel();
    in_test(|may| {
        let attempt = Attempt::begin(&place.bundle, may).unwrap();
        let refused = copy_in(&work, &attempt, &|_| {}, &stopped, may);
        assert!(matches!(refused, Err(Halt::Failed(Failure::NotKoyos(_)))), "{refused:?}");
        assert!(attempt.image().is_file(), "the image that was refused isn't the one downloaded");
        assert!(!attempt.mount().exists(), "an image that isn't Koyo's was mounted");
        assert!(!attempt.staged().exists(), "the app was copied out of an image that isn't Koyo's");
        attempt.discard(may);
    });
    assert!(matches!(place.refused(release), Failure::NotKoyos(_)));
}

#[test]
fn an_image_signed_under_another_identifier_is_refused() {
    let place = Place::new("update-install-other");
    let release = place.holds(&Made::release("9.0.0"), Some("codes.koyo.other"));
    assert!(matches!(place.refused(release), Failure::NotKoyos(_)));
}

#[test]
fn an_image_changed_after_it_was_signed_is_refused() {
    let place = Place::new("update-install-flipped");
    let release = place.holds(&Made::release("9.0.0"), Some(IDENTIFIER));
    let mut bytes = std::fs::read(place.image()).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xff;
    std::fs::write(place.image(), bytes).unwrap();
    assert!(matches!(place.refused(release), Failure::NotKoyos(_)));
}

#[test]
fn an_image_that_holds_another_release_is_refused() {
    let place = Place::new("update-install-version");
    let release = place.holds(&Made::release("8.0.0"), Some(IDENTIFIER));
    assert_eq!(
        place.refused(release),
        Failure::NotOffered { holds: "8.0.0".parse().unwrap() }
    );
}

#[test]
fn a_release_for_a_newer_macos_than_the_feed_said_is_refused() {
    let place = Place::new("update-install-macos");
    let made = Made { minimum_macos: "14.1", ..Made::release("9.0.0") };
    let release = place.holds(&made, Some(IDENTIFIER));
    assert_eq!(place.refused(release), Failure::NeedsMacos("14.1".parse().unwrap()));
}

#[test]
fn a_bundle_in_a_folder_that_cannot_be_written_fails_before_anything_downloads() {
    let place = Place::new("update-install-read-only");
    let beside = place.bundle.parent().unwrap();
    std::fs::set_permissions(beside, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (ended, said) = place.installs(fixtures::release("9.0.0", 1));
    std::fs::set_permissions(beside, std::fs::Permissions::from_mode(0o755)).unwrap();
    let folder = beside.to_owned();
    assert_eq!(ended.unwrap_err(), Halt::Failed(Failure::ReadOnly { folder }));
    assert!(said.is_empty(), "something was fetched: {said:?}");
}

#[test]
fn a_release_that_is_not_in_the_bucket_says_the_download_failed() {
    let place = Place::new("update-install-missing");
    assert!(matches!(place.refused(fixtures::release("9.0.0", 1)), Failure::Download(_)));
}

#[test]
fn an_image_larger_than_the_feed_said_is_not_taken() {
    let place = Place::new("update-install-larger");
    let release = place.holds(&Made::release("9.0.0"), Some(IDENTIFIER));
    let smaller = Release { bytes: release.bytes - 1, ..release };
    assert!(matches!(place.refused(smaller), Failure::Download(_)));
}

#[test]
fn a_job_dropped_mid_download_stops_and_leaves_nothing() {
    let place = Place::new("update-install-stopped");
    let release = place.holds(&Made::release("9.0.0"), Some(IDENTIFIER));
    let (stop, stopped): (Sender<()>, _) = channel();
    let fetch = Folder::at(&place.bucket);
    *fetch.midway.lock().unwrap() = Some(Box::new(move || drop(stop)));

    let (ended, said) = place.runs(&place.work(release, fetch), &stopped);

    assert_eq!(ended.unwrap_err(), Halt::Stopped);
    assert!(matches!(said[..], [Progress::Received(_)]), "{said:?}");
    assert_eq!(fixtures::attempts(&place.bundle), Vec::<PathBuf>::new());
}

#[test]
fn a_sweep_unmounts_the_image_a_crash_left_open_and_removes_its_attempt() {
    let place = Place::new("update-install-crashed");
    place.holds(&Made::release("9.0.0"), Some(IDENTIFIER));
    in_test(|may| {
        let attempt = Attempt::begin(&place.bundle, may).unwrap();
        std::fs::copy(place.image(), attempt.image()).unwrap();
        fixtures::attach(&attempt.image(), &attempt.mount());
        assert_eq!(version_of(&attempt.mount().join("Koyo.app")), "9.0.0");
        let folder = fixtures::let_go(attempt);

        sweep(&place.bundle, may);

        assert!(!folder.exists(), "an attempt nobody holds stayed, its image still mounted");
    });
}

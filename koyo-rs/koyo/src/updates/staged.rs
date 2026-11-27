//! What makes a copy fit to put in place (`docs/updates.md` 4): it is Koyo's, newer than the
//! Koyo that runs, and runs on this Mac; and, to the app, the release offered.

use std::path::Path;

use super::attempt::Attempt;
use super::feed::Release;
use super::installed::info;
use super::signature::Requirement;
use super::stage::Failure;
use super::version::Version;
use crate::blocking::Blocking;

/// A release copied out into an attempt's folder beside this bundle, and checked. It owns its
/// attempt, whose lock says the staged bundle is somebody's. Only [`Staged::check`] makes
/// one, and the hand-over takes only this, so the app never hands over an unchecked copy.
#[derive(Debug)]
pub struct Staged {
    release: Release,
    attempt: Attempt,
}

impl Staged {
    /// Checks the bundle `attempt` holds, every value read from the copy itself and never
    /// from the feed: it [`may_replace`] this Koyo, and its `CFBundleShortVersionString` is
    /// the release offered, which refuses any other genuine release put behind the feed.
    ///
    /// A copy that isn't fit is discarded with its attempt.
    pub(super) fn check(
        attempt: Attempt,
        release: &Release,
        trust: &Requirement,
        macos: Version,
        may: Blocking<'_>,
    ) -> Result<Staged, Failure> {
        let offered = may_replace(&attempt.staged(), trust, macos, may).and_then(|holds| {
            if holds == release.version {
                Ok(())
            } else {
                Err(Failure::NotOffered { holds })
            }
        });
        match offered {
            Ok(()) => Ok(Staged {
                release: release.clone(),
                attempt,
            }),
            Err(failure) => {
                attempt.discard(may);
                Err(failure)
            }
        }
    }

    /// The release it is, as the feed offered it.
    pub(super) fn release(&self) -> &Release {
        &self.release
    }

    /// Its attempt's folder, which the installer is told.
    pub(super) fn attempt(&self) -> &Path {
        self.attempt.folder()
    }

    /// It was handed to the installer: its attempt stays held until this process ends.
    pub(super) fn handed_over(self) {
        self.attempt.hold_until_the_end();
    }

    /// Removes it with its attempt, and gives back the release it was.
    pub(super) fn discard(self, may: Blocking<'_>) -> Release {
        self.attempt.discard(may);
        self.release
    }
}

/// Whether the bundle at `copy` may replace the Koyo this process is, and the release it
/// holds if it may. Every value is read from the copy itself:
/// 1. it satisfies `trust`, this Koyo's designated requirement, strictly, the code nested in
///    it too;
/// 2. its `CFBundleShortVersionString` is newer than this Koyo's, so a genuine release never
///    gives way to an older one;
/// 3. its `LSMinimumSystemVersion` is at most `macos`, this Mac's.
///
/// The one decision, made by the app of the copy it stages and again by the installer of the
/// bundle it's about to put in place.
pub(super) fn may_replace(
    copy: &Path,
    trust: &Requirement,
    macos: Version,
    may: Blocking<'_>,
) -> Result<Version, Failure> {
    trust.holds_for(copy, may)?;
    let info = info(copy, may).map_err(Failure::Broken)?;
    let version = |key: &str| -> Result<Version, Failure> {
        let written = info.get(key).and_then(plist::Value::as_string);
        let written = written.ok_or_else(|| Failure::Broken(format!("the release names no {key}")))?;
        written.parse().map_err(Failure::Broken)
    };
    let holds = version("CFBundleShortVersionString")?;
    if holds <= Version::RUNNING {
        return Err(Failure::NotNewer { holds });
    }
    let needs = version("LSMinimumSystemVersion")?;
    if needs > macos {
        return Err(Failure::NeedsMacos(needs));
    }
    Ok(holds)
}

#[cfg(test)]
mod tests {
    use super::Staged;
    use crate::blocking::in_test;
    use crate::test_support::{scratch, write};
    use crate::updates::attempt::Attempt;
    use crate::updates::fixtures::{self, Made};
    use crate::updates::stage::Failure;
    use crate::updates::version::Version;

    /// Checks a copy made as `made` says, changed by `change` after it was signed, against an
    /// offer of 9.0.0 on macOS 14.0: how the check went, and whether its attempt is still there.
    fn checked(name: &str, made: &Made, change: impl FnOnce(&std::path::Path)) -> (Result<(), Failure>, bool) {
        let beside = scratch(name);
        let installed = beside.join("Koyo.app");
        in_test(|may| {
            let attempt = Attempt::begin(&installed, may).unwrap();
            let folder = attempt.folder().to_owned();
            fixtures::bundle(&attempt.staged(), made);
            change(&attempt.staged());
            let macos: Version = "14.0".parse().unwrap();
            let trust = fixtures::trust();
            let offered = fixtures::release("9.0.0", 0);
            let staged = Staged::check(attempt, &offered, &trust, macos, may);
            let kept = folder.exists();
            (staged.map(|staged| assert_eq!(staged.attempt(), folder)), kept)
        })
    }

    #[test]
    fn a_copy_signed_as_koyo_of_the_release_offered_is_staged() {
        let (staged, kept) = checked("update-staged", &Made::release("9.0.0"), |_| {});
        assert_eq!(staged, Ok(()));
        assert!(kept, "a staged copy keeps its attempt");
    }

    #[test]
    fn a_copy_of_another_release_is_refused() {
        for holds in ["8.0.0", "9.0.1"] {
            let (staged, kept) = checked("update-staged-other", &Made::release(holds), |_| {});
            let holds = holds.parse().unwrap();
            assert_eq!(staged, Err(Failure::NotOffered { holds }));
            assert!(!kept, "a refused copy is removed with its attempt");
        }
    }

    #[test]
    fn a_copy_no_newer_than_the_koyo_that_runs_is_refused() {
        for holds in [env!("CARGO_PKG_VERSION"), "0.0.1"] {
            let (staged, kept) = checked("update-staged-older", &Made::release(holds), |_| {});
            let holds = holds.parse().unwrap();
            assert_eq!(staged, Err(Failure::NotNewer { holds }));
            assert!(!kept);
        }
    }

    #[test]
    fn a_copy_for_a_newer_macos_is_refused() {
        let made = Made { minimum_macos: "14.1", ..Made::release("9.0.0") };
        let (staged, kept) = checked("update-staged-macos", &made, |_| {});
        assert_eq!(staged, Err(Failure::NeedsMacos("14.1".parse().unwrap())));
        assert!(!kept);
    }

    #[test]
    fn a_copy_changed_after_it_was_signed_is_refused() {
        let (staged, kept) = checked("update-staged-edited", &Made::release("9.0.0"), |copy| {
            let plist = fixtures::info_plist(&Made { minimum_macos: "10.0", ..Made::release("9.0.0") });
            write(copy, "Contents/Info.plist", plist.as_bytes());
        });
        assert!(matches!(staged, Err(Failure::NotKoyos(_))), "{staged:?}");
        assert!(!kept);
    }

    #[test]
    fn a_copy_signed_as_something_else_or_not_at_all_is_refused() {
        let other = Made { identifier: Some("codes.koyo.other"), ..Made::release("9.0.0") };
        let unsigned = Made { identifier: None, ..Made::release("9.0.0") };
        for made in [other, unsigned] {
            let (staged, kept) = checked("update-staged-unsigned", &made, |_| {});
            assert!(matches!(staged, Err(Failure::NotKoyos(_))), "{staged:?}");
            assert!(!kept);
        }
    }
}

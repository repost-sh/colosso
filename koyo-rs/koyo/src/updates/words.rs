//! What the person reads about updating (`docs/updates.md` 1), written once for the top bar
//! and About.

use std::time::SystemTime;

use gpui_kit::SharedString;

use super::feed::Release;
use super::installed::Unreleased;
use super::stage::{Checked, Failure, Stage, Step};
use super::version::Version;
use crate::ago::ago;

/// What to show. The top bar shows `Offer` and `Working`; About shows all three. Each says
/// all there is to know in one text: a line, then what a click does, the failure, or when it
/// last checked, as a tooltip and About say it.
#[derive(Clone, Debug, PartialEq)]
pub enum Shown {
    /// Nothing to do: up to date, checking, a failed check, a macOS too old, or a build that
    /// doesn't update itself.
    Quiet(SharedString),
    /// One click to make, whose label is `act`: "Update Koyo", "Restart to Update", "Retry
    /// Update". `failed` when the install went wrong, which the views colour.
    Offer {
        said: SharedString,
        act: &'static str,
        failed: bool,
    },
    /// Under way. `percent` is how much of the download has come, as its ring shows it, or
    /// none while it's checked, copied or handed over.
    Working {
        said: SharedString,
        percent: Option<u8>,
    },
}

impl Shown {
    pub fn said(&self) -> &SharedString {
        match self {
            Shown::Quiet(said) | Shown::Offer { said, .. } | Shown::Working { said, .. } => said,
        }
    }
}

/// What a megabyte is where sizes are said: as Finder counts them.
const MB: u64 = 1_000_000;

/// What `stage` has to say at `now` on macOS `macos`. Pure.
pub(super) fn shown(stage: &Stage, now: SystemTime, macos: Version) -> Shown {
    match stage {
        Stage::Current(checked) => Shown::Quiet(current(checked, now)),
        Stage::NeedsMacos(release) => Shown::Quiet(
            format!(
                "Koyo {} needs macOS {} or later; this Mac has {}.",
                release.version,
                release.minimum_macos.as_macos(),
                macos.as_macos()
            )
            .into(),
        ),
        Stage::Available(release) => Shown::Offer {
            said: format!(
                "Koyo {} is available. Koyo downloads it, then restarts to install it.",
                release.version
            )
            .into(),
            act: "Update Koyo",
            failed: false,
        },
        Stage::Installing { release, step, .. } => working(release, *step),
        Stage::Ready(staged) => Shown::Offer {
            said: format!(
                "Koyo {} is ready. It installs when Koyo restarts or quits.",
                staged.release().version
            )
            .into(),
            act: "Restart to Update",
            failed: false,
        },
        Stage::Restarting(version) => Shown::Working {
            said: format!("Restarting into Koyo {version}.").into(),
            percent: None,
        },
        Stage::Failed { release, failure } => Shown::Offer {
            said: format!("Couldn't update to Koyo {}. {}", release.version, why(failure)).into(),
            act: "Retry Update",
            failed: true,
        },
    }
}

/// What a build that doesn't update itself says of that.
pub(super) fn unreleased(why: &Unreleased) -> Shown {
    Shown::Quiet(SharedString::from(match why {
        Unreleased::NoFeed => {
            "This build doesn't update itself: it was built here, not released.".to_owned()
        }
        Unreleased::NotBundled => {
            "This build doesn't update itself: it doesn't run from an app bundle.".to_owned()
        }
        Unreleased::Unsigned(why) => sentence(&format!(
            "This build doesn't update itself: its own signature can't be read ({why})"
        )),
    }))
}

fn current(checked: &Checked, now: SystemTime) -> SharedString {
    match checked {
        Checked::Not => "Checking for updates…".into(),
        Checked::At(at) => {
            let seconds = now.duration_since(*at).map_or(0, |since| since.as_secs());
            format!("Koyo is up to date. Checked {}.", ago(seconds)).into()
        }
        Checked::Failed { why } => format!(
            "{} Koyo tries again within the hour.",
            sentence(&format!("Couldn't check for updates: {why}"))
        )
        .into(),
    }
}

fn working(release: &Release, step: Step) -> Shown {
    let version = release.version;
    match step {
        Step::Downloading { received } => Shown::Working {
            said: format!(
                "Downloading Koyo {version}: {} of {} MB.",
                received / MB,
                release.bytes.div_ceil(MB)
            )
            .into(),
            percent: Some(if release.bytes == 0 {
                0
            } else {
                ((received as f32 / release.bytes as f32).clamp(0., 1.) * 100.).round() as u8
            }),
        },
        Step::Preparing => Shown::Working {
            said: format!("Checking Koyo {version} and copying it beside this one.").into(),
            percent: None,
        },
    }
}

/// Why an install failed, in what the person can do about it.
fn why(failure: &Failure) -> String {
    match failure {
        Failure::ReadOnly { folder } => format!(
            "Koyo can't replace itself in {}. Quit Koyo, move it to Applications and open it \
             from there.",
            folder.display()
        ),
        Failure::Download(why) => sentence(&format!("The download didn't arrive: {why}")),
        Failure::NotKoyos(why) => sentence(&format!(
            "The download isn't signed as Koyo is, so it wasn't installed: {why}"
        )),
        Failure::NotNewer { holds } => format!(
            "The download holds Koyo {holds}, which is no newer than this one, so it wasn't \
             installed."
        ),
        Failure::NotOffered { holds } => {
            format!("The download holds Koyo {holds}, not the release offered, so it wasn't installed.")
        }
        Failure::NeedsMacos(needs) => format!(
            "The download needs macOS {} or later, so it wasn't installed.",
            needs.as_macos()
        ),
        Failure::Broken(why) => sentence(why),
    }
}

/// `said` as a sentence: a tool's words often end without a full stop.
fn sentence(said: &str) -> String {
    let said = said.trim();
    if said.ends_with(['.', '!', '?', '…']) {
        said.to_owned()
    } else {
        format!("{said}.")
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{Shown, shown, unreleased};
    use crate::updates::feed::Release;
    use crate::updates::fixtures::read;
    use crate::updates::installed::Unreleased;
    use crate::updates::stage::{Checked, Failure, Stage, Step};
    use crate::updates::version::Version;

    fn version(text: &str) -> Version {
        text.parse().unwrap()
    }

    fn release() -> Release {
        Release {
            version: version("0.2.0"),
            bytes: 253_600_000,
            minimum_macos: version("13.0"),
        }
    }

    fn says(stage: &Stage) -> Shown {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        shown(stage, now, version("12.7"))
    }

    #[test]
    fn a_release_found_is_offered_with_what_the_click_does() {
        let shown = says(&Stage::Available(release()));
        assert_eq!(
            read(&shown),
            (
                "Update Koyo",
                "Koyo 0.2.0 is available. Koyo downloads it, then restarts to install it.".into()
            )
        );
        assert!(matches!(shown, Shown::Offer { failed: false, .. }));
    }

    #[test]
    fn nothing_newer_says_when_the_feed_last_answered() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(700);
        assert_eq!(
            read(&says(&Stage::Current(Checked::At(at)))),
            ("quiet", "Koyo is up to date. Checked 5 min ago.".into())
        );
        assert_eq!(
            read(&says(&Stage::Current(Checked::Not))),
            ("quiet", "Checking for updates…".into())
        );
        let failed = Checked::Failed {
            why: "curl: (6) Could not resolve host: dl.koyo.codes".into(),
        };
        assert_eq!(
            read(&says(&Stage::Current(failed))),
            (
                "quiet",
                "Couldn't check for updates: curl: (6) Could not resolve host: dl.koyo.codes. \
                 Koyo tries again within the hour."
                    .into()
            )
        );
    }

    #[test]
    fn a_download_says_how_far_it_has_come() {
        let shown = super::working(&release(), Step::Downloading { received: 120_400_000 });
        assert_eq!(read(&shown), ("working", "Downloading Koyo 0.2.0: 120 of 254 MB.".into()));
        assert!(matches!(shown, Shown::Working { percent: Some(47), .. }), "{shown:?}");
        let shown = super::working(&release(), Step::Preparing);
        assert_eq!(
            read(&shown),
            ("working", "Checking Koyo 0.2.0 and copying it beside this one.".into())
        );
        assert!(matches!(shown, Shown::Working { percent: None, .. }));
    }

    #[test]
    fn a_release_for_a_newer_macos_says_which_and_what_this_mac_has() {
        let release = Release {
            minimum_macos: version("13.0"),
            ..release()
        };
        assert_eq!(
            read(&says(&Stage::NeedsMacos(release))),
            (
                "quiet",
                "Koyo 0.2.0 needs macOS 13.0 or later; this Mac has 12.7.".into()
            )
        );
    }

    #[test]
    fn a_handed_over_release_says_koyo_is_restarting() {
        let shown = says(&Stage::Restarting(version("0.2.0")));
        assert_eq!(read(&shown), ("working", "Restarting into Koyo 0.2.0.".into()));
        assert_eq!(shown, Shown::Working { said: shown.said().clone(), percent: None });
    }

    #[test]
    fn a_failure_says_what_to_do_about_it_and_offers_to_try_again() {
        let failed = |failure| {
            let stage = Stage::Failed { release: release(), failure };
            let shown = says(&stage);
            assert!(matches!(shown, Shown::Offer { failed: true, .. }));
            read(&shown)
        };
        assert_eq!(
            failed(Failure::ReadOnly { folder: "/Volumes/Koyo".into() }),
            (
                "Retry Update",
                "Couldn't update to Koyo 0.2.0. Koyo can't replace itself in /Volumes/Koyo. Quit \
                 Koyo, move it to Applications and open it from there."
                    .into()
            )
        );
        assert_eq!(
            failed(Failure::Download("curl: (22) The requested URL returned error: 404".into())).1,
            "Couldn't update to Koyo 0.2.0. The download didn't arrive: curl: (22) The requested \
             URL returned error: 404."
        );
        assert_eq!(
            failed(Failure::NotKoyos("code object is not signed at all".into())).1,
            "Couldn't update to Koyo 0.2.0. The download isn't signed as Koyo is, so it wasn't \
             installed: code object is not signed at all."
        );
        assert_eq!(
            failed(Failure::NotNewer { holds: version("0.1.0") }).1,
            "Couldn't update to Koyo 0.2.0. The download holds Koyo 0.1.0, which is no newer than \
             this one, so it wasn't installed."
        );
        assert_eq!(
            failed(Failure::NotOffered { holds: version("0.1.5") }).1,
            "Couldn't update to Koyo 0.2.0. The download holds Koyo 0.1.5, not the release \
             offered, so it wasn't installed."
        );
        assert_eq!(
            failed(Failure::NeedsMacos(version("15.0"))).1,
            "Couldn't update to Koyo 0.2.0. The download needs macOS 15.0 or later, so it wasn't \
             installed."
        );
        assert_eq!(
            failed(Failure::Broken("hdiutil: attach failed - no mountable file systems".into())).1,
            "Couldn't update to Koyo 0.2.0. hdiutil: attach failed - no mountable file systems."
        );
    }

    #[test]
    fn a_build_that_does_not_update_itself_says_why() {
        assert_eq!(
            read(&unreleased(&Unreleased::NoFeed)).1,
            "This build doesn't update itself: it was built here, not released."
        );
        assert_eq!(
            read(&unreleased(&Unreleased::NotBundled)).1,
            "This build doesn't update itself: it doesn't run from an app bundle."
        );
        assert_eq!(
            read(&unreleased(&Unreleased::Unsigned("code object is not signed at all".into()))).1,
            "This build doesn't update itself: its own signature can't be read (code object is \
             not signed at all)."
        );
    }
}

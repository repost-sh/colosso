//! Where an update stands, and how the feed's answer moves it (`docs/updates.md` 3). Pure.

use std::path::PathBuf;
use std::time::SystemTime;

use super::feed::{Answer, Release};
use super::install::Job;
use super::staged::Staged;
use super::version::Version;

/// Where the update stands. One value, owned by `Updates`.
pub(super) enum Stage {
    /// Nothing newer: not asked yet, or the feed names this version or an older one.
    Current,
    /// A newer release this Mac can run.
    Available(Release),
    /// A newer release that needs a newer macOS than this Mac has. Not offered; About says why.
    NeedsMacos(Release),
    /// The person clicked: the job downloads, checks and stages it. Holds the job, so the job
    /// lives exactly as long as this stage, and leaving the stage any other way stops it.
    Installing {
        release: Release,
        step: Step,
        _job: Job,
    },
    /// Checked and staged beside this bundle; put in place when the app ends, however it ends.
    Ready(Staged),
    /// Handed to the installer, which opens Koyo again once it's in place: the app is ending.
    Restarting(Version),
    /// The install failed; the one click starts it again.
    Failed { release: Release, failure: Failure },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Downloading { received: u64 },
    Preparing,
}

/// When the feed last answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Checked {
    Not,
    At(SystemTime),
    Failed { why: String },
}

/// Why an install failed, in what the person can do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The folder the bundle is in can't be written: a mounted disk image, the read-only copy
    /// macOS runs a quarantined app from, an Applications folder of another user's.
    ReadOnly { folder: PathBuf },
    /// curl's words.
    Download(String),
    /// The image or the copy isn't signed by the identity that signed this Koyo, or isn't
    /// whole: the system's words.
    NotKoyos(String),
    /// Koyo's, but no newer than the Koyo that runs: the release it holds.
    NotNewer { holds: Version },
    /// Koyo's and newer, but another release than the one offered: the one it holds.
    NotOffered { holds: Version },
    /// It needs this macOS or later.
    NeedsMacos(Version),
    /// Mounting, copying, reading or handing over went wrong: the tool's or the system's
    /// words.
    Broken(String),
}

/// The stage once the feed has answered `answer`. An install under way, staged or handed over
/// is never disturbed by a check; a failure gives way only to a newer release than the one
/// that failed.
pub(super) fn heard(stage: Stage, answer: Answer) -> Stage {
    match (stage, answer) {
        (Stage::Current | Stage::Available(_) | Stage::NeedsMacos(_), answer) => match answer {
            Answer::Newer(release) => Stage::Available(release),
            Answer::NeedsMacos(release) => Stage::NeedsMacos(release),
            Answer::Current => Stage::Current,
        },
        (Stage::Failed { release, .. }, Answer::Newer(newer)) if newer.version > release.version => {
            Stage::Available(newer)
        }
        (
            stage @ (Stage::Failed { .. }
            | Stage::Installing { .. }
            | Stage::Ready(_)
            | Stage::Restarting(_)),
            _,
        ) => stage,
    }
}

#[cfg(test)]
mod tests {
    use super::{Failure, Stage, heard};
    use crate::updates::feed::{Answer, Release};

    fn release(version: &str) -> Release {
        Release {
            version: version.parse().unwrap(),
            bytes: 1,
            minimum_macos: "13.0".parse().unwrap(),
        }
    }

    fn failed(version: &str) -> Stage {
        Stage::Failed {
            release: release(version),
            failure: Failure::Download("no route".into()),
        }
    }

    /// What a stage is, for the stages a check can reach.
    fn named(stage: &Stage) -> String {
        match stage {
            Stage::Current => "current".into(),
            Stage::Available(release) => format!("available {}", release.version),
            Stage::NeedsMacos(release) => format!("needs macos {}", release.version),
            Stage::Failed { release, .. } => format!("failed {}", release.version),
            Stage::Restarting(version) => format!("restarting {version}"),
            Stage::Installing { .. } | Stage::Ready(_) => "at work".into(),
        }
    }

    #[test]
    fn a_check_moves_the_stages_with_nothing_under_way() {
        for stage in [
            || Stage::Current,
            || Stage::Available(release("0.2.0")),
            || Stage::NeedsMacos(release("0.2.0")),
        ] {
            let after = heard(stage(), Answer::Newer(release("0.3.0")));
            assert_eq!(named(&after), "available 0.3.0");
            let after = heard(stage(), Answer::NeedsMacos(release("0.3.0")));
            assert_eq!(named(&after), "needs macos 0.3.0");
            assert_eq!(named(&heard(stage(), Answer::Current)), "current");
        }
    }

    #[test]
    fn a_failure_gives_way_only_to_a_newer_release() {
        let after = heard(failed("0.2.0"), Answer::Newer(release("0.3.0")));
        assert_eq!(named(&after), "available 0.3.0");
        let after = heard(failed("0.2.0"), Answer::Newer(release("0.2.0")));
        assert_eq!(named(&after), "failed 0.2.0");
        let after = heard(failed("0.2.0"), Answer::NeedsMacos(release("0.3.0")));
        assert_eq!(named(&after), "failed 0.2.0");
        assert_eq!(named(&heard(failed("0.2.0"), Answer::Current)), "failed 0.2.0");
    }

    #[test]
    fn a_release_handed_over_is_left_alone() {
        let restarting = || Stage::Restarting("0.2.0".parse().unwrap());
        for answer in [
            Answer::Newer(release("0.3.0")),
            Answer::NeedsMacos(release("0.3.0")),
            Answer::Current,
        ] {
            assert_eq!(named(&heard(restarting(), answer)), "restarting 0.2.0");
        }
    }
}

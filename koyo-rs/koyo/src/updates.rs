//! Koyo updating itself (`docs/updates.md`). A released bundle names its feed in its
//! `Info.plist`; Koyo reads it at launch and hourly, and when a newer Koyo this Mac can run is
//! out, the top bar offers it. One click downloads the release's disk image into a folder of
//! the attempt's own beside the bundle, checks it's signed as this Koyo is, copies the app out,
//! checks the copy, and restarts: unsaved edits and pages are asked about as at a quit, the
//! staged release is handed to `koyo install-update`, and the app ends. The installer waits
//! for it to be gone, checks for itself that the staged bundle may replace the one it runs
//! from, exchanges the two in one step, and opens it. A staged release is put in place however
//! Koyo quits. A bundle made here names no feed and never checks.
//!
//! The views (`shell/update_button.rs`, `settings_view/about.rs`) ask one question, [`shown`],
//! and call one verb, [`act`]. Nothing here draws, and the restart reaches `quitting` as an
//! action, so no import of this module closes a cycle.
//!
//! Two Koyos run from one bundle are not designed for: each stages in a folder of its own and
//! leaves the other's alone, but an installer that replaces the bundle while the other still
//! runs has that one start its engine's helpers from the new release.

mod attempt;
mod feed;
mod fetch;
#[cfg(test)]
mod fixtures;
mod hand_over;
mod install;
mod installed;
mod signature;
mod stage;
mod staged;
#[cfg(test)]
mod tests;
mod version;
mod words;

use std::mem;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task, actions};

use crate::blocking::{self, Blocking};
use feed::Answer;
use fetch::{Curl, Fetch};
use install::{Job, Progress};
use installed::{Installed, Unreleased};
use stage::{Checked, Failure, Stage, Step};
use version::Version;

pub use hand_over::{INSTALL_UPDATE, main};
pub use words::Shown;

/// A released Koyo over a bucket that is a folder, for the tests of the views that show it.
#[cfg(test)]
pub mod testing {
    pub use super::fixtures::{Koyo, koyo, until_it_says};
}

actions!(
    updates,
    [
        /// Restarts Koyo into the release staged beside it. Dispatched here, once a release is
        /// staged and at a click on Restart to Update; `quitting` answers it.
        RestartToUpdate
    ]
);

/// How often the feed is read while Koyo runs, after the read at launch.
const EVERY: Duration = Duration::from_hours(1);

/// The update's one owner: what this copy of Koyo is, where the update stands, and the work
/// under way. Exists only for a release run from its bundle, so no stage is ever held for a
/// build that can't install one.
pub struct Updates {
    installed: Installed,
    /// Where the update stands. Every surface draws from this alone, through `words::shown`.
    stage: Stage,
    fetch: Arc<dyn Fetch>,
    /// Removes what an earlier run left, then reads the feed now and every [`EVERY`].
    _looking: Task<()>,
}

/// What this copy is, kept for the app's lifetime.
enum TheUpdates {
    Unreleased(Unreleased),
    Released(Entity<Updates>),
}

impl Global for TheUpdates {}

/// Finds what this copy of Koyo is (`installed::find`). A release starts looking for newer
/// ones and, as the app quits, hands a staged one over.
pub fn init(cx: &mut App, may: Blocking<'_>) {
    let updates = match installed::find(may) {
        Ok(installed) => TheUpdates::Released(released(installed, Arc::new(Curl), cx)),
        Err(why) => {
            log::info!("updates: {}", words::unreleased(&why).said());
            TheUpdates::Unreleased(why)
        }
    };
    cx.set_global(updates);
}

/// The updates of the release `installed` is, read through `fetch`.
fn released(installed: Installed, fetch: Arc<dyn Fetch>, cx: &mut App) -> Entity<Updates> {
    log::info!(
        "updates: Koyo {} at {} reads {}",
        Version::RUNNING,
        installed.bundle.display(),
        installed.feed.address()
    );
    let updates = cx.new(|cx| Updates::new(installed, fetch, cx));
    let quitting = updates.clone();
    blocking::on_quit(cx, move |may, cx| {
        quitting.update(cx, |updates, _| updates.quitting(may));
    });
    updates
}

/// The one click, from the top bar or About: installs the release found, tries again after a
/// failure, or restarts into the staged release. Does nothing in any other stage, so a second
/// click before the first has redrawn anything is nothing.
pub fn act(cx: &mut App) {
    if let Some(updates) = observed(cx) {
        updates.update(cx, Updates::act);
    }
}

/// What the top bar and About say, from the stage.
pub fn shown(cx: &App) -> Shown {
    match cx.global::<TheUpdates>() {
        TheUpdates::Unreleased(why) => words::unreleased(why),
        TheUpdates::Released(updates) => {
            let updates = updates.read(cx);
            words::shown(&updates.stage, SystemTime::now(), updates.installed.macos)
        }
    }
}

/// The entity a view observes to redraw as the update moves; none for a build that doesn't
/// update itself.
pub fn observed(cx: &App) -> Option<Entity<Updates>> {
    match cx.global::<TheUpdates>() {
        TheUpdates::Unreleased(_) => None,
        TheUpdates::Released(updates) => Some(updates.clone()),
    }
}

/// Hands the staged release to the installer to put in place and open once this Koyo has
/// ended, and says whether the installer started: `quitting`'s, which then ends the app. Not
/// started, the update has failed, the app stays open, and the answer is no; with nothing
/// staged the answer is no too.
pub fn hand_over_to_reopen(cx: &mut App) -> Task<bool> {
    match observed(cx) {
        Some(updates) => updates.update(cx, Updates::hand_over_to_reopen),
        None => Task::ready(false),
    }
}

impl Updates {
    fn new(installed: Installed, fetch: Arc<dyn Fetch>, cx: &mut Context<Self>) -> Self {
        let (bundle, feed, macos) = (installed.bundle.clone(), installed.feed.clone(), installed.macos);
        let reading = fetch.clone();
        let looking = cx.spawn(async move |updates, cx| {
            let executor = cx.background_executor().clone();
            blocking::spawn(&executor, move |may| attempt::sweep(&bundle, may)).await;
            loop {
                let (fetch, feed) = (reading.clone(), feed.clone());
                let look = move |may: Blocking<'_>| feed::look(&*fetch, &feed, macos, may);
                let answer = blocking::spawn(&executor, look).await;
                let heard = |updates: &mut Self, cx: &mut Context<Self>| {
                    updates.heard(answer, SystemTime::now(), cx);
                };
                if updates.update(cx, heard).is_err() {
                    return;
                }
                executor.timer(EVERY).await;
            }
        });
        Updates {
            installed,
            stage: Stage::Current(Checked::Not),
            fetch,
            _looking: looking,
        }
    }

    /// The feed answered, or didn't.
    fn heard(&mut self, answer: Result<Answer, String>, now: SystemTime, cx: &mut Context<Self>) {
        match &answer {
            Ok(Answer::Newer(release)) => {
                log::info!("updates: the feed offers Koyo {}", release.version);
            }
            Ok(Answer::NeedsMacos(release)) => log::info!(
                "updates: the feed offers Koyo {}, which needs macOS {}",
                release.version,
                release.minimum_macos.as_macos()
            ),
            Ok(Answer::Current) => log::info!("updates: the feed has nothing newer"),
            Err(why) => log::warn!("updates: the feed didn't answer: {why}"),
        }
        let was = mem::replace(&mut self.stage, Stage::Current(Checked::Not));
        self.stage = stage::heard(was, answer, now);
        cx.notify();
    }

    fn act(&mut self, cx: &mut Context<Self>) {
        match mem::replace(&mut self.stage, Stage::Current(Checked::Not)) {
            Stage::Available(release) | Stage::Failed { release, .. } => {
                let job = Job::start(release.clone(), &self.installed, self.fetch.clone(), cx);
                self.stage = match job {
                    Ok(job) => Stage::Installing {
                        release,
                        step: Step::Downloading { received: 0 },
                        _job: job,
                    },
                    Err(failure) => Stage::Failed { release, failure },
                };
                cx.notify();
            }
            ready @ Stage::Ready(_) => {
                self.stage = ready;
                restart(cx);
            }
            other => self.stage = other,
        }
    }

    /// The install job said how far it is. A staged release restarts Koyo: the click said
    /// "update", and updating ends in a restart.
    fn moved(&mut self, progress: Progress, cx: &mut Context<Self>) {
        let Stage::Installing { release, step, .. } = &mut self.stage else {
            return;
        };
        match progress {
            Progress::Received(received) => *step = Step::Downloading { received },
            Progress::Preparing => *step = Step::Preparing,
            Progress::Done(Ok(staged)) => {
                self.stage = Stage::Ready(staged);
                restart(cx);
            }
            Progress::Done(Err(failure)) => {
                let release = release.clone();
                self.stage = Stage::Failed { release, failure };
            }
        }
        cx.notify();
    }

    fn hand_over_to_reopen(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        let staged = match mem::replace(&mut self.stage, Stage::Current(Checked::Not)) {
            Stage::Ready(staged) => staged,
            other => {
                self.stage = other;
                return Task::ready(false);
            }
        };
        self.stage = Stage::Restarting(staged.release().version);
        cx.notify();
        let exe = self.installed.exe.clone();
        let handing = blocking::spawn(cx.background_executor(), move |may| {
            hand_over::hand_over(staged, &exe, true, may)
        });
        cx.spawn(async move |updates, cx| {
            let Err((release, why)) = handing.await else {
                return true;
            };
            log::error!("updates: {why}");
            let failed = |updates: &mut Self, cx: &mut Context<Self>| {
                let failure = Failure::Broken(why);
                updates.stage = Stage::Failed { release, failure };
                cx.notify();
            };
            updates.update(cx, failed).ok();
            false
        })
    }

    /// The app is ending. A staged release is handed to the installer, which puts it in place
    /// once this process is gone and opens nothing: a quit, not a restart. One already handed
    /// over to reopen is the installer's. An install under way is abandoned: its curl ends
    /// with the app, and the next launch removes the folder nobody holds. Runs on the main
    /// thread as the app quits.
    fn quitting(&mut self, may: Blocking<'_>) {
        match mem::replace(&mut self.stage, Stage::Current(Checked::Not)) {
            Stage::Ready(staged) => {
                let handed = hand_over::hand_over(staged, &self.installed.exe, false, may);
                if let Err((_, why)) = handed {
                    log::error!("updates: {why}");
                }
            }
            other => self.stage = other,
        }
    }
}

/// Asks `quitting` to restart Koyo into the staged release, once whatever window the click
/// came through is done with it.
fn restart(cx: &mut Context<Updates>) {
    cx.defer(|cx| cx.dispatch_action(&RestartToUpdate));
}

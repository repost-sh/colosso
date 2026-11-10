//! An app being opened (`docs/agent-desktop.md` 5): started hidden as an instance of its own,
//! assigned to the agent desktop, shown there, and answered once its windows are up. Never an
//! instance the person runs: one of theirs that the launch came back with is left untouched.

use std::time::{Duration, Instant};

use koyo_desk_wire::link::{Answer, Refusal, RefusalCode, RequestId, Window};

use crate::app::{self, App};
use crate::apps::{self, Launch};
use crate::host::{Host, no_desktop};
use crate::input::Poster;
use crate::ns::Obj;
use crate::{ns, skylight, windows};

/// How long Launch Services gets, the window server to take the assignment, an app to show
/// its first window, and that window's company to follow it.
const STARTS_WITHIN: Duration = Duration::from_secs(30);
const ASSIGNS_WITHIN: Duration = Duration::from_secs(3);
const SHOWS_WITHIN: Duration = Duration::from_secs(6);
const SETTLES: Duration = Duration::from_millis(400);
/// An app that shows no window is told it's active after this long, and opened once more,
/// as a click on its Dock icon would, after that long: some apps show their window only then.
const NUDGED_AFTER: Duration = Duration::from_millis(800);
const REOPENED_AFTER: Duration = Duration::from_millis(1800);

/// Apps that hand a second launch to the instance that runs, which would open a window where
/// the person is: not started while they run one. Chromium and Electron apps do the same and
/// are given a data folder of their own instead ([`app::own_profile`]).
const HANDS_OVER: [&str; 4] = [
    "company.thebrowser.Browser",
    "org.mozilla.firefox",
    "com.apple.finder",
    "com.apple.systempreferences",
];

pub struct Opening {
    pub request: RequestId,
    /// The caller went away: the app is still taken in, and nobody is answered.
    pub canceled: bool,
    /// The app as it was asked for.
    asked: String,
    bundle: Option<String>,
    /// The instances the person ran when it was asked.
    theirs: Vec<i32>,
    /// Whether an instance of its own was asked of Launch Services, or the agent's running
    /// one was to take it.
    fresh: bool,
    launch: Launch,
    started: Instant,
    pid: Option<i32>,
    assigned: bool,
    /// When the app came, then when it was shown.
    since: Instant,
    /// How many windows it has, and since when.
    seen: (usize, Instant),
    /// Where the app is, and what was tried to make it show a window.
    url: Obj,
    nudge: Nudge,
}

/// What an app that shows no window was done to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Nudge {
    Nothing,
    ToldActive,
    Reopened,
}

fn persons(asked: &str) -> Refusal {
    Refusal::new(
        RefusalCode::AppIsThePersons,
        format!(
            "{asked} is running as the person's and takes no second instance: use another app, \
             or ask the person to quit theirs"
        ),
    )
}

impl Host {
    /// Starts opening `app`; the answer comes from [`Host::open_along`].
    pub fn open(
        &mut self,
        request: RequestId,
        asked: &str,
        document: Option<&str>,
        arguments: &[String],
    ) -> Option<Result<Answer, Refusal>> {
        let Some(url) = apps::find(asked) else {
            return Some(Err(Refusal::new(
                RefusalCode::NotFound,
                format!(
                    "no app “{asked}”: give its name as in Applications, its bundle id or its path"
                ),
            )));
        };
        let bundle = apps::bundle_id(&url);
        // Koyo itself isn't for agents to drive: a second Koyo on the person's data would
        // let an agent grant itself what the person didn't.
        if (bundle.as_deref()).is_some_and(|bundle| bundle.starts_with("dev.koyo.")) {
            return Some(Err(Refusal::new(
                RefusalCode::Invalid,
                "Koyo isn't opened on the agent desktop: drive it with its own tools",
            )));
        }
        if let Some(file) = document
            && !file.contains("://")
            && !file.starts_with('/')
        {
            return Some(Err(Refusal::new(
                RefusalCode::Invalid,
                format!("“{file}” isn't a full path or a URL: give the file's whole path"),
            )));
        }
        let ours = self.pids();
        let running = bundle.as_deref().map(apps::running).unwrap_or_default();
        let theirs: Vec<i32> = (running.iter().copied())
            .filter(|pid| !ours.contains(pid))
            .collect();
        let mine = self.staying(&running);
        // Asked again with nothing to open, the instance that runs is the answer: shown if
        // it's hidden, and asked for a window if it has none (as a click on a Dock icon asks),
        // unless the person runs the app too and Launch Services could ask theirs.
        if let (Some(pid), None, true) = (mine, document, arguments.is_empty()) {
            let shows = |host: &Host| {
                (host.desktop.as_ref())
                    .is_some_and(|desktop| !windows::on(desktop.space, &[pid]).is_empty())
            };
            if !shows(self) {
                self.front.acted(pid);
                if let Some(app) = self.apps.iter().find(|app| app.pid == pid) {
                    apps::unhide(&app.running);
                }
                if theirs.is_empty() {
                    apps::start(&url, None, &[], false);
                }
                ns::pump_until(Duration::from_secs(3), || shows(self));
            }
            return Some(Ok(self.opened(pid)));
        }
        let profile = app::own_profile(&url, bundle.as_deref(), &self.state);
        let hands_over = bundle
            .as_deref()
            .is_some_and(|bundle| HANDS_OVER.contains(&bundle));
        if !theirs.is_empty() && hands_over {
            return Some(Err(persons(asked)));
        }
        let mut arguments = arguments.to_vec();
        if let Some(profile) = profile {
            // The agent's instance keeps to its own data folder, whatever is asked: with the
            // person's, the launch would be handed to their instance.
            arguments.retain(|argument| !argument.starts_with("--user-data-dir"));
            arguments.push(format!("--user-data-dir={profile}"));
            arguments.push("--no-first-run".to_owned());
            arguments.push("--no-default-browser-check".to_owned());
        }
        // A document goes to the agent's instance when it's the only one running; with the
        // person's running too, Launch Services could pick theirs, so a new one is started.
        let fresh = mine.is_none() || !theirs.is_empty();
        let now = Instant::now();
        self.openings.push(Opening {
            request,
            canceled: false,
            asked: asked.to_owned(),
            bundle,
            theirs,
            fresh,
            launch: apps::start(&url, document, &arguments, fresh),
            started: now,
            pid: None,
            assigned: false,
            since: now,
            seen: (0, now),
            url,
            nudge: Nudge::Nothing,
        });
        None
    }

    /// The agent's own among `running` that isn't on its way out: an instance asked to quit a
    /// moment ago isn't one to open in.
    fn staying(&self, running: &[i32]) -> Option<i32> {
        (running.iter().copied())
            .find(|pid| (self.apps.iter()).any(|app| app.pid == *pid && app.quitting.is_none()))
    }

    /// The answer for the app `pid`: its windows as they're listed now.
    fn opened(&mut self, pid: i32) -> Answer {
        self.list();
        let windows: Vec<Window> = (self.windows.iter())
            .filter(|window| window.pid == pid)
            .cloned()
            .collect();
        let windows = self.local(&windows);
        let app = (self.apps.iter())
            .find(|app| app.pid == pid)
            .map(|app| app.name.clone())
            .unwrap_or_default();
        Answer::Opened { app, pid, windows }
    }

    /// Moves every opening along and answers the ones that are done.
    pub fn open_along(&mut self) {
        if self.openings.is_empty() {
            return;
        }
        let mut openings = std::mem::take(&mut self.openings);
        openings.retain_mut(|opening| match self.advance(opening) {
            Some(result) => {
                if !opening.canceled {
                    self.answer(opening.request, result);
                }
                false
            }
            None => true,
        });
        openings.append(&mut self.openings);
        self.openings = openings;
    }

    fn advance(&mut self, opening: &mut Opening) -> Option<Result<Answer, Refusal>> {
        let Some(space) = self.desktop.as_ref().map(|desktop| desktop.space) else {
            return Some(Err(no_desktop(&self.missing)));
        };
        if opening.pid.is_none() {
            let launched = match opening.launch.take() {
                None if opening.started.elapsed() > STARTS_WITHIN => {
                    let message = format!("{} didn't start in time", opening.asked);
                    return Some(Err(Refusal::new(RefusalCode::Failed, message)));
                }
                None => return None,
                Some(Err(why)) => {
                    let message = format!("{} didn't start: {why}", opening.asked);
                    return Some(Err(Refusal::new(RefusalCode::Failed, message)));
                }
                Some(Ok(launched)) => launched,
            };
            // Launch Services came back with an instance of the person's: not ours to touch.
            // So is any instance but the agent's own, when its own was to take the launch:
            // the person may have started theirs since.
            let known = self.apps.iter().any(|app| app.pid == launched.pid);
            if opening.theirs.contains(&launched.pid) || (!opening.fresh && !known) {
                return Some(Err(persons(&opening.asked)));
            }
            opening.pid = Some(launched.pid);
            opening.since = Instant::now();
            if known {
                opening.assigned = true;
            } else {
                self.apps.push(App::new(launched));
                self.apps_changed();
            }
        }
        let pid = opening.pid?;
        // Whatever it does while it starts is the start's doing.
        self.front.acted(pid);
        let Some(app) = (self.apps.iter()).find(|app| app.pid == pid && !app.ended()) else {
            return Some(self.ended_early(opening));
        };
        if !opening.assigned {
            if skylight::assign(pid, space) {
                opening.assigned = true;
                opening.since = Instant::now();
                apps::unhide(&app.running);
            } else if opening.since.elapsed() > ASSIGNS_WITHIN {
                apps::quit(&app.running, true);
                let message = format!("{} couldn't be put on the agent desktop", opening.asked);
                return Some(Err(Refusal::new(RefusalCode::Failed, message)));
            }
            return None;
        }
        let count = windows::on(space, &[pid]).len();
        // An app hides itself once it has finished starting, which can be after it was
        // shown: it's shown again until its windows are up.
        if count == 0 && apps::hidden(&app.running) {
            apps::unhide(&app.running);
        } else if count == 0 {
            let waited = opening.since.elapsed();
            if waited >= NUDGED_AFTER && opening.nudge == Nudge::Nothing {
                opening.nudge = Nudge::ToldActive;
                Poster::tell_active(pid);
            }
            // Launch Services asks the instance that runs to open again; with one of the
            // person's running too it could ask theirs, so only without.
            let alone = opening.theirs.is_empty();
            if waited >= REOPENED_AFTER && opening.nudge == Nudge::ToldActive && alone {
                opening.nudge = Nudge::Reopened;
                apps::start(&opening.url, None, &[], false);
            }
        }
        if count != opening.seen.0 {
            opening.seen = (count, Instant::now());
        }
        let settled = count > 0 && opening.seen.1.elapsed() >= SETTLES;
        if settled || opening.since.elapsed() >= SHOWS_WITHIN {
            return Some(Ok(self.opened(pid)));
        }
        None
    }

    /// The answer for an app that went before it showed a window.
    fn ended_early(&mut self, opening: &Opening) -> Result<Answer, Refusal> {
        let running = (opening.bundle.as_deref())
            .map(apps::running)
            .unwrap_or_default();
        let ours = self.pids();
        // It handed over to an instance of the agent's own, as a Chromium app does.
        if let Some(pid) = self.staying(&running) {
            return Ok(self.opened(pid));
        }
        if running.iter().any(|pid| !ours.contains(pid)) {
            return Err(persons(&opening.asked));
        }
        // Or to one that was closing: gone with it.
        let message = if running.is_empty() {
            format!("{} quit right after starting", opening.asked)
        } else {
            format!("{} was still closing: open it again", opening.asked)
        };
        Err(Refusal::new(RefusalCode::Failed, message))
    }
}

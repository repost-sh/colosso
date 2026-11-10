//! What the host keeps and does turn by turn (`docs/agent-desktop.md` 3): it hears what the
//! app says, keeps the desktop's windows listed and captured, and moves along what takes
//! time (an app starting, a screenshot, a wait).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use koyo_desk_wire::Rect;
use koyo_desk_wire::link::{
    Answer, DesktopState, FromHost, LogLevel, Refusal, RefusalCode, RequestId, ToHost, Window,
};
use objc2_core_foundation::{CGPoint, CGSize};

use crate::app::App;
use crate::apps;
use crate::ax::Element;
use crate::capture::Streams;
use crate::desktop::{self, Desktop, Started};
use crate::front::FrontGuard;
use crate::grants::Grants;
use crate::input::{Poster, Target};
use crate::link::Link;
use crate::opening::Opening;
use crate::shot::Shots;
use crate::windows;
use crate::{ns, skylight};

/// How often the windows are listed, and the grants read again; and how often while the
/// person drags in the tab and a moment after, when a window follows their pointer.
const LISTED_EVERY: Duration = Duration::from_millis(200);
const LISTED_LIVELY: Duration = Duration::from_millis(16);
const LIVELY_FOR: Duration = Duration::from_millis(500);
const GRANTS_EVERY: Duration = Duration::from_secs(2);
/// And while one is missing, so the person sees it taken the moment they give it.
const GRANTS_AWAITED: Duration = Duration::from_millis(300);
/// How long an app of the agent's gets to quit when asked.
const QUITS_WITHIN: Duration = Duration::from_secs(3);

/// Text being typed, a few characters a turn of the loop, so the loop goes on hearing the
/// app: the person's input stops it.
pub struct Typing {
    pub request: RequestId,
    pub target: Target,
    pub rest: std::collections::VecDeque<char>,
}

/// A wait for the desktop to go still.
pub struct Waiting {
    pub request: RequestId,
    pub quiet: Duration,
    pub limit: Duration,
    pub started: Instant,
}

pub struct Host {
    pub link: Link,
    pub state: PathBuf,
    pub desktop: Option<Desktop>,
    /// Why there's no desktop.
    pub missing: String,
    pub grants: Grants,
    pub streams: Option<Streams>,
    pub shots: Shots,
    pub poster: Poster,
    pub front: FrontGuard,
    pub apps: Vec<App>,
    pub openings: Vec<Opening>,
    pub waits: Vec<Waiting>,
    /// Texts being typed, the first under way.
    pub typings: std::collections::VecDeque<Typing>,
    /// The desktop's windows as last listed, front to back.
    pub windows: Vec<Window>,
    /// The desktop's own picture (its wallpaper): shown behind them, never acted on.
    backdrop: Option<Window>,
    /// The windows other processes keep for those, and draw their content in.
    pub hosted: Vec<windows::Hosted>,
    helpers: windows::Helpers,
    /// Whether the app shows the desktop's tab.
    pub visible: bool,
    /// The window keys go to: the one last clicked.
    pub focus: Option<u32>,
    /// The window its app was last told is key.
    pub keyed: Option<u32>,
    /// The window the person's button went down in, until it comes up.
    pub grab: Option<Target>,
    /// When the person's button last went down, dragged or came up.
    pub lively: Option<Instant>,
    pub keys: crate::keyboard::Guard,
    /// The window the person holds by its bar.
    pub moving: Option<crate::moving::Moving>,
    /// The last snapshot's elements, and the number the next snapshot's refs start at.
    pub refs: HashMap<String, Element>,
    pub next_ref: u64,
    /// When the windows or their picture last changed.
    pub changed: Instant,
    changes_seen: u64,
    returned_seen: u64,
    /// Asked for with `--stats`: when the frames were last said, and how many had been sent
    /// and had come by then.
    pub stats: Option<(Instant, u64, u64)>,
    listed: Instant,
    pub grants_read: Instant,
    ending: bool,
}

impl Host {
    pub fn new(link: Link, state: PathBuf, grants: Grants, poster: Poster) -> Host {
        let now = Instant::now();
        Host {
            link,
            state,
            desktop: None,
            missing: String::new(),
            grants,
            streams: None,
            shots: Shots::default(),
            poster,
            front: FrontGuard::new(),
            apps: Vec::new(),
            openings: Vec::new(),
            waits: Vec::new(),
            typings: std::collections::VecDeque::new(),
            windows: Vec::new(),
            backdrop: None,
            hosted: Vec::new(),
            helpers: windows::Helpers::default(),
            visible: false,
            focus: None,
            keyed: None,
            grab: None,
            lively: None,
            keys: crate::keyboard::Guard::default(),
            moving: None,
            refs: HashMap::new(),
            next_ref: 1,
            changed: now,
            changes_seen: 0,
            returned_seen: 0,
            stats: None,
            listed: now,
            grants_read: now,
            ending: false,
        }
    }

    /// Whether something is under way that the loop moves along turn by turn, with no
    /// waiting between turns.
    pub fn busy(&self) -> bool {
        !self.typings.is_empty()
    }

    /// Whether the app said quit or went away.
    pub fn ending(&self) -> bool {
        self.ending || self.link.closed()
    }

    pub fn tell(&mut self, message: &FromHost) {
        self.link.tell(message);
    }

    pub fn log(&mut self, level: LogLevel, text: impl Into<String>) {
        let text = text.into();
        self.tell(&FromHost::Log { level, text });
    }

    pub fn answer(&mut self, request: RequestId, result: Result<Answer, Refusal>) {
        self.tell(&FromHost::Answer { request, result });
    }

    fn tell_desktop(&mut self) {
        let state = match &self.desktop {
            Some(desktop) => DesktopState::Ready {
                size: desktop.size(),
                scale: desktop.scale,
            },
            None => DesktopState::Missing {
                why: self.missing.clone(),
            },
        };
        self.tell(&FromHost::Desktop { state });
    }

    pub fn make_desktop(&mut self) {
        let started = Instant::now();
        match Desktop::make(&self.state) {
            Ok(desktop) => {
                let took = started.elapsed().as_millis();
                self.log(
                    LogLevel::Info,
                    format!(
                        "made the agent desktop (desktop {}) in {took} ms",
                        desktop.space
                    ),
                );
                // Apps that outlived a desktop the person removed go to the new one.
                for app in &self.apps {
                    skylight::assign(app.pid, desktop.space);
                }
                if let Some(streams) = &mut self.streams {
                    streams.scale = desktop.scale;
                }
                self.front.set_desktop(desktop.space);
                self.desktop = Some(desktop);
            }
            Err(why) => self.missing = why,
        }
        self.tell_desktop();
    }

    /// The pids of the agent's apps.
    pub fn pids(&self) -> Vec<i32> {
        self.apps.iter().map(|app| app.pid).collect()
    }

    /// Says which apps are the agent's, to the guard of the person's front and to the record
    /// a later run cleans up by.
    pub fn apps_changed(&mut self) {
        self.front.set_ours(self.pids());
        // A window looked at before may be a helper's of an app that came since.
        self.helpers = windows::Helpers::default();
        let started: Vec<Started> = (self.apps.iter())
            .map(|app| Started {
                pid: app.pid,
                at: app.started,
            })
            .collect();
        desktop::record_apps(&self.state, &started);
    }

    /// Windows as the app is told them: counted from the desktop's top left.
    pub fn local(&self, windows: &[Window]) -> Vec<Window> {
        let Some(desktop) = &self.desktop else {
            return windows.to_vec();
        };
        (windows.iter())
            .map(|window| Window {
                frame: desktop.local(window.frame),
                ..window.clone()
            })
            .collect()
    }

    /// Lists the desktop's windows, tells the app when they changed, and keeps the capture
    /// in step.
    pub fn list(&mut self) {
        self.listed = Instant::now();
        let before = self.apps.len();
        self.apps.retain(|app| !app.ended());
        if self.apps.len() != before {
            self.apps_changed();
        }
        let Some(desktop) = &self.desktop else {
            return;
        };
        let (space, bounds) = (desktop.space, desktop.display);
        // The person can remove a desktop in Mission Control: its windows would show on
        // theirs, so another is made and the apps put there.
        let desktops = skylight::desktops();
        if !desktops.is_empty() && !desktops.contains(&space) {
            self.desktop = None;
            self.missing = "the agent desktop was removed".into();
            self.log(
                LogLevel::Warn,
                "the agent desktop was removed; making another",
            );
            self.make_desktop();
            return;
        }
        for app in &mut self.apps {
            app.watch();
            for window in app.take_created() {
                keep_inside(&window, bounds);
            }
        }
        let (listed, backdrop) = windows::with_backdrop(space, &self.pids(), bounds);
        self.hosted = self.helpers.hosted(&listed, &self.pids());
        let changed = listed != self.windows;
        if changed {
            let ids: Vec<u32> = listed.iter().map(|window| window.id).collect();
            for app in &mut self.apps {
                app.keep_only(&ids);
            }
            self.windows = listed;
            self.changed = Instant::now();
        }
        if changed || backdrop != self.backdrop {
            self.backdrop = backdrop;
            let windows = self.local(&self.windows);
            let backdrop = (self.backdrop.as_ref())
                .and_then(|backdrop| self.local(std::slice::from_ref(backdrop)).pop());
            self.tell(&FromHost::Windows { windows, backdrop });
        }
        self.capture();
    }

    /// Keeps the windows' streams: fast while the tab shows or a wait watches, slow otherwise.
    pub fn capture(&mut self) {
        let watched = self.visible || !self.waits.is_empty();
        if let Some(streams) = &mut self.streams {
            // Every window is captured as long as it's there, slowly while nobody looks:
            // one that isn't is hidden to its app (`capture::UNWATCHED_RATE`).
            if self.grants.screen_recording {
                streams.sync(&self.windows, self.backdrop.as_ref(), watched);
            } else {
                streams.sync(&[], None, watched);
            }
            let changes = streams.changes.load(Ordering::Relaxed);
            if changes != self.changes_seen {
                self.changes_seen = changes;
                self.changed = Instant::now();
            }
        }
    }

    fn waits(&mut self) {
        let quiet_for = self.changed.elapsed();
        let mut done = Vec::new();
        self.waits.retain(|wait| {
            let waited = wait.started.elapsed();
            let still = quiet_for >= wait.quiet && waited >= wait.quiet;
            if still || waited >= wait.limit {
                done.push((wait.request, still, waited));
                return false;
            }
            true
        });
        for (request, still, waited) in done {
            let millis = u64::try_from(waited.as_millis()).unwrap_or(u64::MAX);
            self.answer(request, Ok(Answer::Waited { still, millis }));
        }
    }

    fn quits(&mut self) {
        for app in &self.apps {
            if app
                .quitting
                .is_some_and(|since| since.elapsed() > QUITS_WITHIN)
            {
                apps::quit(&app.running, true);
            }
        }
    }

    pub fn tick(&mut self) {
        let awaited = !(self.grants.accessibility && self.grants.screen_recording);
        if self.grants_read.elapsed()
            >= if awaited {
                GRANTS_AWAITED
            } else {
                GRANTS_EVERY
            }
        {
            self.read_grants();
        }
        self.open_along();
        self.type_along();
        let lively = self
            .lively
            .is_some_and(|since| since.elapsed() < LIVELY_FOR);
        if self.listed.elapsed() >= if lively { LISTED_LIVELY } else { LISTED_EVERY } {
            self.list();
        } else {
            self.capture();
        }
        for (request, result) in self.shots.poll() {
            self.answer(request, result);
        }
        self.guard_keys();
        self.waits();
        self.quits();
        self.say_frames();
        for asked in self.front.take_asked() {
            if let Some((app, bundle)) = apps::named(&asked) {
                self.tell(&FromHost::Wanted { app, bundle });
            }
        }
        let returned = self.front.returned();
        if returned != self.returned_seen {
            self.returned_seen = returned;
            self.log(
                LogLevel::Info,
                format!(
                    "an agent's app came to the front; gave it back to the person's ({returned})"
                ),
            );
        }
    }

    pub fn hear(&mut self, message: ToHost) {
        match message {
            ToHost::Hello { .. } => {}
            ToHost::Visible { visible } => {
                self.visible = visible;
                self.capture();
            }
            ToHost::Input { input } => {
                self.stop_typing();
                self.person(&input);
            }
            ToHost::Request { request, ask } => {
                if let Some(result) = self.ask(request, ask) {
                    self.answer(request, result);
                }
            }
            ToHost::Cancel { request } => {
                self.waits.retain(|wait| wait.request != request);
                self.typings.retain(|typing| typing.request != request);
                self.shots.cancel(request);
                for opening in &mut self.openings {
                    if opening.request == request {
                        opening.canceled = true;
                    }
                }
            }
            ToHost::OpenForPerson { bundle } => {
                apps::start_for_person(&bundle);
            }
            ToHost::Quit => self.ending = true,
        }
    }

    /// Closes the agent's apps and removes the desktop.
    pub fn end(mut self) {
        self.streams = None;
        for app in &self.apps {
            apps::quit(&app.running, false);
        }
        ns::pump_until(QUITS_WITHIN, || self.apps.iter().all(App::ended));
        for app in self.apps.iter().filter(|app| !app.ended()) {
            apps::quit(&app.running, true);
        }
        ns::pump_until(Duration::from_secs(1), || self.apps.iter().all(App::ended));
        if let Some(desktop) = self.desktop.take() {
            let started = Instant::now();
            desktop.remove(&self.state);
            let took = started.elapsed().as_millis();
            self.log(
                LogLevel::Info,
                format!("removed the agent desktop in {took} ms"),
            );
        }
    }
}

/// Moves a window that was made off the desktop back onto it.
fn keep_inside(window: &Element, bounds: Rect) {
    let Some(frame) = window.frame() else { return };
    let kept = frame.kept_inside(bounds);
    if kept == frame {
        return;
    }
    window.set_point(
        "AXPosition",
        CGPoint {
            x: kept.x,
            y: kept.y,
        },
    );
    if (kept.width, kept.height) != (frame.width, frame.height) {
        let size = CGSize {
            width: kept.width,
            height: kept.height,
        };
        window.set_size("AXSize", size);
    }
}

/// A refusal for an ask that needs the desktop.
pub fn no_desktop(why: &str) -> Refusal {
    let why = if why.is_empty() {
        "it isn't made yet"
    } else {
        why
    };
    Refusal::new(
        RefusalCode::NoDesktop,
        format!("the agent desktop isn't there: {why}"),
    )
}

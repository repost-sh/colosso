//! Keeping the front the person's (`docs/agent-desktop.md` 2): an app of the agent's that
//! makes itself the front app (an editor at launch, an app answering a document) is answered
//! by giving the front back to the app the person had, 12-15 ms later as measured. Not while
//! the person is on the agent desktop itself: there, an agent's app in front is their doing.
//! Which app the person had is asked of Launch Services at that moment (`order`): the one
//! the workspace's notices named last is only what's fallen back on, as it gave the front to
//! the app before when the person had switched apps a few milliseconds earlier.
//!
//! The notices come on a queue of their own, not on the host's loop: macOS follows an app
//! that makes itself the front one to the desktop its windows are on, and a front given
//! back late doesn't stop that. Given back 140 ms late, because the loop was held up in an
//! act, it left the person on the agent's desktop. Should macOS go there all the same, the
//! agent's app coming to the front again as it arrives is answered once more, which brings
//! the person back to the desktop their own app is on.
//!
//! One activation isn't the app's own doing: the person opening an app (its Dock icon,
//! Spotlight) whose only running instance is the agent's. Launch Services then activates the
//! agent's, which nobody sees. The host can't tell that from an app calling for attention
//! by itself, so it starts nothing on a guess: an activation that no act of the agent's
//! explains is told to Koyo, which says so to the person and offers them their own.

use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

use crate::ns::{self, Obj};
use crate::skylight::{self, SpaceId};

/// How long after an agent's act on an app its coming to the front is the act's doing.
const EXPLAINED_FOR: Duration = Duration::from_millis(1500);
/// How long after the front was given back macOS may still arrive on the agent's desktop,
/// following the app that had taken it.
const FOLLOWS_WITHIN: Duration = Duration::from_secs(1);

/// A running app, handed between the queue the notices come on and the host's loop.
struct Running(Obj);

// SAFETY: an `NSRunningApplication`, which its documentation says any thread may use.
unsafe impl Send for Running {}

struct Watch {
    /// The agent's apps, and the desktop they're on.
    ours: Vec<i32>,
    desktop: Option<SpaceId>,
    /// When the agent last acted on each of its apps.
    acted: HashMap<i32, Instant>,
    /// The agent's apps the person seems to have asked for, for the host to answer.
    asked: Vec<Running>,
    /// The app the person last had in front.
    persons: Option<Running>,
    /// How often the front was given back, and when it last was from another desktop than
    /// the agent's.
    returned: u64,
    given: Option<Instant>,
}

/// Watches the workspace's activations for as long as it lives.
pub struct FrontGuard {
    watch: Arc<Mutex<Watch>>,
    center: Obj,
    token: Obj,
    /// The queue the notices come on, for as long as they're watched.
    _queue: Obj,
}

fn held(watch: &Mutex<Watch>) -> MutexGuard<'_, Watch> {
    watch.lock().unwrap_or_else(PoisonError::into_inner)
}

fn front() -> Option<Obj> {
    // SAFETY: the shared workspace and its front app, nil when there's none.
    unsafe {
        let workspace: Obj = msg_send![class!(NSWorkspace), sharedWorkspace];
        msg_send![&*workspace, frontmostApplication]
    }
}

/// The app the person has in front behind the agent's, as Launch Services orders the apps
/// now; none when it can't say.
fn persons_now(ours: &[i32]) -> Option<Obj> {
    let pid = crate::order::persons_among(&crate::order::pids(), ours)?;
    // SAFETY: a class method answering the running app of a pid, nil when it's gone.
    unsafe {
        msg_send![
            class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ]
    }
}

fn pid_of(app: &AnyObject) -> i32 {
    // SAFETY: a running app answers its pid.
    unsafe { msg_send![app, processIdentifier] }
}

impl FrontGuard {
    /// Starts watching. Notices come on a queue of the guard's own, so the front is given
    /// back at once whatever the host's loop is held up in.
    pub fn new() -> FrontGuard {
        let watch = Arc::new(Mutex::new(Watch {
            ours: Vec::new(),
            desktop: None,
            acted: HashMap::new(),
            asked: Vec::new(),
            persons: front().map(Running),
            returned: 0,
            given: None,
        }));
        let seen = Arc::clone(&watch);
        let activated = RcBlock::new(move |notice: NonNull<AnyObject>| {
            // SAFETY: the notice's user info holds the app that was activated under the
            // workspace's own key.
            let app: Option<Obj> = unsafe {
                let info: Option<Obj> = msg_send![notice.as_ref(), userInfo];
                info.and_then(|info| ns::entry(&info, "NSWorkspaceApplicationKey"))
            };
            let Some(app) = app else { return };
            let mut watch = held(&seen);
            if !watch.ours.contains(&pid_of(&app)) {
                watch.persons = Some(Running(app));
                return;
            }
            // On the agent's desktop an agent's app in front is the person's doing, unless
            // macOS brought them there itself, after the app the front was just taken from.
            let there = watch.desktop.is_some() && skylight::active() == watch.desktop;
            let followed = (watch.given).is_some_and(|given| given.elapsed() < FOLLOWS_WITHIN);
            if there && !followed {
                return;
            }
            if let Some(persons) = persons_now(&watch.ours) {
                watch.persons = Some(Running(persons));
            }
            let given = watch.persons.as_ref().is_some_and(|Running(persons)| {
                // SAFETY: a running app answers `activateWithOptions:`.
                unsafe { msg_send![&**persons, activateWithOptions: 0usize] }
            });
            if given {
                watch.returned += 1;
            }
            if !there {
                watch.given = Some(Instant::now());
            }
            let explained = (watch.acted.get(&pid_of(&app)))
                .is_some_and(|acted| acted.elapsed() < EXPLAINED_FOR);
            if !explained {
                watch.asked.push(Running(app));
            }
            // The host's loop says what happened, and answers what the person asked for.
            ns::wake();
        });
        // SAFETY: the workspace's notification centre, observed with a block it copies, on
        // a queue that runs one notice at a time; the token is kept to stop observing.
        let (center, token, queue) = unsafe {
            let workspace: Obj = msg_send![class!(NSWorkspace), sharedWorkspace];
            let center: Obj = msg_send![&*workspace, notificationCenter];
            let queue: Obj = msg_send![class!(NSOperationQueue), new];
            let _: () = msg_send![&*queue, setMaxConcurrentOperationCount: 1isize];
            let token: Obj = msg_send![
                &*center,
                addObserverForName: &*ns::string("NSWorkspaceDidActivateApplicationNotification"),
                object: std::ptr::null::<AnyObject>(),
                queue: &*queue,
                usingBlock: &*activated
            ];
            (center, token, queue)
        };
        FrontGuard {
            watch,
            center,
            token,
            _queue: queue,
        }
    }

    /// Says which processes are the agent's.
    pub fn set_ours(&self, pids: Vec<i32>) {
        held(&self.watch).ours = pids;
    }

    /// Says the agent just acted on its app `pid`, or is starting it: the app coming to the
    /// front in the next moment is that act's doing.
    pub fn acted(&self, pid: i32) {
        held(&self.watch).acted.insert(pid, Instant::now());
    }

    /// The agent's apps that came to the front with no act of the agent's to explain it:
    /// the person asked for them.
    pub fn take_asked(&self) -> Vec<Obj> {
        let asked = std::mem::take(&mut held(&self.watch).asked);
        asked.into_iter().map(|Running(app)| app).collect()
    }

    /// Says which desktop is the agent's.
    pub fn set_desktop(&self, desktop: SpaceId) {
        held(&self.watch).desktop = Some(desktop);
    }

    pub fn returned(&self) -> u64 {
        held(&self.watch).returned
    }
}

impl Drop for FrontGuard {
    fn drop(&mut self) {
        // SAFETY: the centre the token came from.
        let _: () = unsafe { msg_send![&*self.center, removeObserver: &*self.token] };
    }
}

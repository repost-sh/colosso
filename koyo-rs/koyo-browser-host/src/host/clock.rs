//! The pages' frame clock. Chromium starts each frame of a page on screen when the display
//! refreshes, as the host tells it (`external_begin_frame_enabled`), rather than by a clock of
//! its own: left to its own, an off-screen page's clock fell to half the display's rate once
//! the pointer first moved over the page, and stayed there. The display's link ticks on a
//! thread of its own; each tick has the UI thread begin a frame of every page on screen,
//! unless the last tick's turn hasn't come yet. A page that hasn't finished its last frame
//! waits for it (CEF answers the next begin with nothing until then).
//!
//! The link runs while any page is shown, and stops on the first tick that finds none, so the
//! host sleeps while every page is out of sight; a page shown again starts it.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use cef::{ImplBrowser, ImplBrowserHost, ImplTask, Task, ThreadId, WrapTask, rc::Rc, wrap_task};

use super::with;

/// A tick's turn on the UI thread is under way or due.
static DUE: AtomicBool = AtomicBool::new(false);

/// The display's link: made the first time a page opens, and never freed; none without a
/// display.
static LINK: OnceLock<Option<Link>> = OnceLock::new();

struct Link(NonNull<c_void>);

// SAFETY: a display link is started and stopped from any thread.
unsafe impl Send for Link {}
// SAFETY: as above; the pointer itself never changes.
unsafe impl Sync for Link {}

type Output = unsafe extern "C" fn(
    *mut c_void,
    *const c_void,
    *const c_void,
    u64,
    *mut u64,
    *mut c_void,
) -> i32;

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CVDisplayLinkCreateWithActiveCGDisplays(link: *mut *mut c_void) -> i32;
    fn CVDisplayLinkSetOutputCallback(
        link: *mut c_void,
        output: Output,
        context: *mut c_void,
    ) -> i32;
    fn CVDisplayLinkStart(link: *mut c_void) -> i32;
    fn CVDisplayLinkStop(link: *mut c_void) -> i32;
    fn CVDisplayLinkIsRunning(link: *mut c_void) -> u8;
}

fn link() -> Option<&'static Link> {
    LINK.get_or_init(|| {
        let mut link = std::ptr::null_mut();
        // SAFETY: an out-pointer for the new link, which is never freed; the callback takes no
        // context.
        let made = unsafe {
            CVDisplayLinkCreateWithActiveCGDisplays(&raw mut link) == 0
                && !link.is_null()
                && CVDisplayLinkSetOutputCallback(link, tick, std::ptr::null_mut()) == 0
        };
        made.then(|| NonNull::new(link).map(Link)).flatten()
    })
    .as_ref()
}

/// Starts the clock, unless it runs already: whether it runs. Without one (no display), pages
/// keep Chromium's own clock.
pub(crate) fn start() -> bool {
    link().is_some_and(|link| {
        // SAFETY: the link lives for good.
        unsafe {
            CVDisplayLinkIsRunning(link.0.as_ptr()) != 0 || CVDisplayLinkStart(link.0.as_ptr()) == 0
        }
    })
}

/// Stops the clock: no page is shown.
fn stop() {
    if let Some(link) = link() {
        // SAFETY: the link lives for good.
        unsafe { CVDisplayLinkStop(link.0.as_ptr()) };
    }
}

/// The display refreshed: the UI thread's turn, unless one is due already.
unsafe extern "C" fn tick(
    _: *mut c_void,
    _: *const c_void,
    _: *const c_void,
    _: u64,
    _: *mut u64,
    _: *mut c_void,
) -> i32 {
    if !DUE.swap(true, Ordering::AcqRel) {
        let mut begin = BeginFrames::new();
        cef::post_task(ThreadId::UI, Some(&mut begin));
    }
    0
}

wrap_task! {
    struct BeginFrames;

    impl Task {
        fn execute(&self) {
            DUE.store(false, Ordering::Release);
            // A page shown whose browser is still to come keeps the clock running for it.
            let hosts = with(|host| {
                (host.panes.values().any(|pane| pane.shown)).then(|| {
                    (host.panes.values())
                        .filter(|pane| pane.shown)
                        .filter_map(|pane| pane.browser.as_ref()?.host())
                        .collect::<Vec<_>>()
                })
            })
            .flatten();
            let Some(hosts) = hosts else {
                stop();
                return;
            };
            for host in hosts {
                host.send_external_begin_frame();
            }
        }
    }
}

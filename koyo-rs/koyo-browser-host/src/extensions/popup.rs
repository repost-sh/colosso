//! Where an extension's popup shows (`docs/extensions.md` 6). The popup is Chrome's own: a
//! bubble it opens under the extensions button of the page's toolbar, which a page of Koyo's
//! carries hidden, so left alone the bubble sits at the page's top right corner. The app says
//! where the icon pressed is, and the bubble is put there, its top right corner under the
//! icon's, and kept there: Chrome anchors it to its toolbar again each time the popup's page
//! resizes.
//!
//! Chrome makes the bubble's window before the popup's page has loaded and shows it once it
//! has. Until it's placed a new bubble is clear, so it never flashes at the page's corner.
//! Nothing says which window is the popup's but the window itself: a new one of Chromium's
//! bubble class, a child of the page's holder, or over it where Chromium gives it no parent.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::CStr;
use std::time::{Duration, Instant};

use koyo_browser_ipc::PaneId;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSScreen, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect};

/// The class of the windows Chromium's own bubbles and dialogs are.
const BUBBLE: &CStr = c"NativeWidgetMacNSWindow";
/// How long an action may take to show a popup: one without (`action.onClicked`) shows none.
const WAIT: Duration = Duration::from_secs(5);
/// How often a popup's place is looked at.
const EVERY: f64 = 1. / 120.;
/// The clear margin Chrome leaves round a bubble for its shadow, in points.
const MARGIN: f64 = 7.;
/// A bubble no wider than this hasn't heard its page's size yet (Chrome's least is 25).
const UNSIZED: f64 = 25. + 2. * MARGIN;
/// How long a bubble that stays that small is waited for before it's shown as it is.
const SIZING: Duration = Duration::from_millis(250);

struct Expected {
    /// The page the action ran on, whose holder the popup is a child of.
    pane: PaneId,
    /// Where the popup hangs from: its top right corner, in screen points from the bottom
    /// left.
    at: NSPoint,
    /// The windows there were before the action ran.
    before: HashSet<isize>,
    /// When to stop waiting for a popup.
    until: Instant,
    /// The popup, once it showed, and when.
    popup: Option<(Retained<NSWindow>, Instant)>,
    /// Whether the popup is in its place and shown.
    placed: bool,
    /// New bubbles kept clear until one of them is the popup.
    hidden: Vec<Retained<NSWindow>>,
}

thread_local! {
    static EXPECTED: RefCell<Option<Expected>> = const { RefCell::new(None) };
}

/// An action is about to run on `pane`'s page: a popup it opens hangs from `at`.
pub(super) fn expect(pane: PaneId, at: [f64; 2]) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let before = windows(mtm).iter().map(|window| window.windowNumber()).collect();
    let was = EXPECTED.with(|expected| {
        expected.borrow_mut().replace(Expected {
            pane,
            at: NSPoint::new(at[0], at[1]),
            before,
            until: Instant::now() + WAIT,
            popup: None,
            placed: false,
            hidden: Vec::new(),
        })
    });
    match was {
        // The one before is let go as it is; the watch goes on for this one.
        Some(was) => reveal(&was.hidden),
        None => crate::embedded::pump::every(EVERY, look),
    }
}

/// Nothing is waited for any more: the engine is stopping.
pub(super) fn forget() {
    if let Some(was) = EXPECTED.with(|expected| expected.borrow_mut().take()) {
        reveal(&was.hidden);
    }
}

fn windows(mtm: MainThreadMarker) -> Vec<Retained<NSWindow>> {
    NSApplication::sharedApplication(mtm).windows().to_vec()
}

fn reveal(hidden: &[Retained<NSWindow>]) {
    for window in hidden {
        crate::native::set_alpha(window, 1.);
    }
}

/// Whether `window` is one of Chromium's bubbles: its class, and no title bar.
fn is_bubble(window: &NSWindow) -> bool {
    crate::native::class_of(window).name() == BUBBLE
        && !window.styleMask().contains(NSWindowStyleMask::Titled)
}

/// Whether `bubble`, a new bubble of Chromium's, is the popup of the page in `holder`.
/// Chromium makes a bubble a child of its page's window, the holder; but only once that
/// window is on the Space shown, so on another desktop the bubble has no parent, and is told
/// by lying over its page.
fn is_popup_of(bubble: &NSWindow, holder: Option<&NSWindow>) -> bool {
    let Some(holder) = holder else {
        return false;
    };
    match bubble.parentWindow() {
        Some(parent) => std::ptr::eq(&raw const *parent, std::ptr::from_ref(holder)),
        None => overlap(bubble.frame(), holder.frame()),
    }
}

/// Whether two rectangles of the screen share any of it.
fn overlap(a: NSRect, b: NSRect) -> bool {
    a.origin.x < b.origin.x + b.size.width
        && b.origin.x < a.origin.x + a.size.width
        && a.origin.y < b.origin.y + b.size.height
        && b.origin.y < a.origin.y + a.size.height
}

/// One look at the popup: found, placed, kept in place, or given up on. Whether to look again.
fn look() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    // Taken out while `AppKit` is called, which may call back into the host.
    let Some(mut expected) = EXPECTED.with(|expected| expected.borrow_mut().take()) else {
        return false;
    };
    let keep = step(&mut expected, mtm);
    if !keep {
        reveal(&expected.hidden);
        return false;
    }
    EXPECTED.with(|slot| {
        let mut slot = slot.borrow_mut();
        // An action run meanwhile has its own expectation, which wins.
        if slot.is_none() {
            *slot = Some(expected);
        } else {
            reveal(&expected.hidden);
        }
    });
    true
}

fn step(expected: &mut Expected, mtm: MainThreadMarker) -> bool {
    let now = Instant::now();
    if let Some((popup, _)) = &expected.popup
        && expected.placed
    {
        // Shown and placed: kept there until it closes.
        if !popup.isVisible() {
            return false;
        }
        put(popup, expected.at, mtm);
        return true;
    }
    if expected.popup.is_none() {
        let holder = crate::native::holder_of(expected.pane);
        for window in windows(mtm) {
            if expected.before.contains(&window.windowNumber()) || !is_bubble(&window) {
                continue;
            }
            if !window.isVisible() {
                // It may become the popup: clear until it's known and placed.
                if crate::native::alpha(&window) >= 1. {
                    crate::native::set_alpha(&window, 0.);
                    expected.hidden.push(window);
                }
                continue;
            }
            if is_popup_of(&window, holder.as_deref()) {
                log::debug!("extensions: popup window {} for pane {}", window.windowNumber(), expected.pane);
                expected.popup = Some((window, now));
                break;
            }
        }
    }
    let Some((popup, since)) = &expected.popup else {
        return now < expected.until;
    };
    // A hidden bubble waits a moment for its page's size rather than show a stub that grows.
    if popup.frame().size.width <= UNSIZED && now.duration_since(*since) < SIZING {
        return true;
    }
    put(popup, expected.at, mtm);
    crate::native::set_alpha(popup, 1.);
    // With the keyboard, as a popup has it in Chrome: it closes when it loses it, which it
    // only does once it had it.
    popup.makeKeyWindow();
    expected.hidden.retain(|window| !std::ptr::eq(&raw const **window, &raw const **popup));
    reveal(&expected.hidden);
    expected.hidden.clear();
    expected.placed = true;
    true
}

/// Puts `popup` with its top right corner at `at`, the margin round it aside, and wholly on
/// the screen `at` is on.
fn put(popup: &NSWindow, at: NSPoint, mtm: MainThreadMarker) {
    let frame = popup.frame();
    let screen = NSScreen::screens(mtm)
        .iter()
        .map(|screen| screen.visibleFrame())
        .find(|screen| contains(*screen, at));
    let origin = place(frame.size.width, frame.size.height, at, screen);
    if (origin.x - frame.origin.x).abs() > 0.5 || (origin.y - frame.origin.y).abs() > 0.5 {
        popup.setFrameOrigin(origin);
    }
}

fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height
}

/// The origin (bottom left) of a bubble `width` by `height`, margin included, whose page's
/// top right corner is at `at`; moved as little as it takes to be wholly on `screen`.
fn place(width: f64, height: f64, at: NSPoint, screen: Option<NSRect>) -> NSPoint {
    let mut x = at.x + MARGIN - width;
    let mut y = at.y + MARGIN - height;
    if let Some(screen) = screen {
        let (left, bottom) = (screen.origin.x, screen.origin.y);
        let (right, top) = (left + screen.size.width, bottom + screen.size.height);
        // The page inside the margin is what has to show.
        x = x.min(right + MARGIN - width).max(left - MARGIN);
        y = y.min(top + MARGIN - height).max(bottom - MARGIN);
    }
    NSPoint::new(x.round(), y.round())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSSize;

    fn screen() -> NSRect {
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(1440., 875.))
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn a_popup_hangs_by_its_top_right_corner() {
        // 300 by 400 of page in a bubble 314 by 414: the page's corner is at the point.
        let origin = place(314., 414., NSPoint::new(1000., 800.), Some(screen()));
        assert!(near(origin.x, 693.) && near(origin.y, 393.));
        assert!(near(origin.x + 314. - MARGIN, 1000.));
        assert!(near(origin.y + 414. - MARGIN, 800.));
    }

    #[test]
    fn rectangles_that_share_ground_overlap() {
        let rect = |x, y, w, h| NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
        let page = rect(100., 100., 800., 600.);
        assert!(overlap(rect(700., 500., 300., 400.), page));
        assert!(overlap(rect(200., 200., 10., 10.), page));
        // Beside it, or only touching its edge, isn't over it.
        assert!(!overlap(rect(900., 100., 50., 50.), page));
        assert!(!overlap(rect(0., 0., 100., 100.), page));
    }

    #[test]
    fn a_popup_stays_on_its_screen() {
        // Wider than what's left of the screen to the icon's left: its left edge at the
        // screen's.
        let origin = place(414., 314., NSPoint::new(200., 800.), Some(screen()));
        assert!(near(origin.x, -MARGIN));
        // Taller than what's under the icon: its foot at the screen's.
        let origin = place(314., 614., NSPoint::new(1000., 300.), Some(screen()));
        assert!(near(origin.y, -MARGIN));
        // With no screen known it's where it was asked.
        let origin = place(414., 314., NSPoint::new(200., 800.), None);
        assert!(near(origin.x, 200. + MARGIN - 414.));
    }
}

//! The window's traffic lights, faded with the top rail they sit on: compact mode has none, as
//! Arc's hasn't. Gone, they're hidden as well, so a click where they were moves the window
//! rather than closing it. In full screen they're left as they are, in the bar `AppKit` drops
//! from the top of the screen.

use gpui_kit::Window;
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow, NSWindowButton, NSWindowStyleMask};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const BUTTONS: [NSWindowButton; 3] = [
    NSWindowButton::CloseButton,
    NSWindowButton::MiniaturizeButton,
    NSWindowButton::ZoomButton,
];

/// Shows `window`'s traffic lights `alpha` of the way, from gone (0) to as `AppKit` draws them
/// (1), touching only what differs.
pub fn fade(window: &Window, alpha: f32) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    let alpha = if ns_window
        .styleMask()
        .contains(NSWindowStyleMask::FullScreen)
    {
        1.
    } else {
        f64::from(alpha)
    };
    let hidden = alpha <= 0.;
    for button in BUTTONS
        .into_iter()
        .filter_map(|kind| ns_window.standardWindowButton(kind))
    {
        if (button.alphaValue() - alpha).abs() > f64::EPSILON {
            button.setAlphaValue(alpha);
        }
        if button.isHidden() != hidden {
            button.setHidden(hidden);
        }
    }
}

/// The `NSWindow` GPUI draws `window` in; none for a test's window.
fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    // `Window` has a `window_handle` of its own, GPUI's.
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: on macOS the handle is GPUI's view for the window, which the window holds for as
    // long as it's open, and `window` is open while it's being drawn.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

//! A GPUI window's `NSWindow`, for what GPUI doesn't do with it: growing a window down from its
//! top, where GPUI's resize keeps the bottom edge, as `AppKit` does; and its traffic lights
//! (`traffic_lights.rs`).

use gpui_kit::{App, AsyncApp, Window};
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Makes `window`'s content `height` high, its top edge staying where it is; false when it has
/// no `NSWindow`, as a test's window hasn't. The frame changes once the app is free of the update
/// asking for it: `AppKit` tells GPUI of a resize as it happens, which can't be heard mid-update.
pub fn fit_height(window: &Window, height: f32, cx: &mut App) -> bool {
    let Some(ns_window) = ns_window(window) else {
        return false;
    };
    let height = f64::from(height);
    cx.spawn(async move |_: &mut AsyncApp| {
        let frame = ns_window.frame();
        let content = ns_window.contentRectForFrameRect(frame);
        let chrome = frame.size.height - content.size.height;
        let mut fitted = frame;
        fitted.size.height = height + chrome;
        // Screen coordinates rise from the bottom: the top stays where it is as the bottom moves.
        fitted.origin.y = frame.origin.y + frame.size.height - fitted.size.height;
        ns_window.setFrame_display(fitted, true);
    })
    .detach();
    true
}

/// The `NSWindow` GPUI draws `window` in; none for a test's window.
pub(super) fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: an `AppKit` handle's view is the window's content view, which `AppKit` keeps for
    // as long as the window is open, and the window is open while it's being drawn in.
    let view: &NSView = unsafe { appkit.ns_view.cast().as_ref() };
    view.window()
}

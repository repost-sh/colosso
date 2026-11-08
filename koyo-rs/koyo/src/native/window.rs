//! A GPUI window's `NSWindow`, for what GPUI doesn't do with it: growing a window down from its
//! top, where GPUI's resize keeps the bottom edge, as `AppKit` does; and its traffic lights
//! (`traffic_lights.rs`).

use gpui_kit::{AnyWindowHandle, App, AsyncApp, Window};
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSViewLayerContentsPlacement, NSWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Makes `window`'s content `height` high, its top edge staying where it is; false when it has
/// no `NSWindow`, as a test's window hasn't. The frame changes once the app is free of the update
/// asking for it: `AppKit` tells GPUI of a resize as it happens, which can't be heard mid-update.
///
/// The frame GPUI drew last is the old height. Left to itself, Core Animation stretches it to the
/// new one until GPUI's next frame, which comes late while the app is busy: a launcher whose rows
/// come and go showed its field squashed or stretched. So the frame is pinned to the top-left
/// rather than stretched, which a view laid out from its top shows right, and drawn again at the
/// new height before the resize reaches the screen, in the same Core Animation transaction.
pub fn fit_height(window: &Window, height: f32, cx: &mut App) -> bool {
    let Some(view) = gpui_view(window) else {
        return false;
    };
    let Some(ns_window) = view.window() else {
        return false;
    };
    view.setLayerContentsPlacement(NSViewLayerContentsPlacement::TopLeft);
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
        // GPUI heard the resize as it happened; its view draws its layer when asked to display it,
        // presenting in the transaction the new frame commits in.
        if let Some(layer) = view.layer() {
            layer.display();
        }
    })
    .detach();
    true
}

/// Has macOS work out `window`'s shadow and edge again from what's drawn in it by now. It works
/// them out from a see-through window's content once, as the window first shows, and keeps them
/// until a resize: drawn before the content had settled, the edge along the window's foot came
/// out ragged, patches of the desktop's blur where the rim should be. So it's asked again once
/// the first frames are in, and once more after, for what comes late.
pub fn refresh_edge(window: AnyWindowHandle, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        for wait in [100, 400, 1000] {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(wait))
                .await;
            // Looked up each time: a card's window may well have gone by now.
            let refreshed = window.update(cx, |_, window, _| {
                if let Some(ns_window) = ns_window(window) {
                    ns_window.invalidateShadow();
                }
            });
            if refreshed.is_err() {
                return;
            }
        }
    })
    .detach();
}

/// The `NSWindow` GPUI draws `window` in; none for a test's window.
pub(crate) fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    gpui_view(window)?.window()
}

/// The view GPUI draws `window` in, its layer GPUI's; none for a test's window.
fn gpui_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: an `AppKit` handle's view is the view GPUI draws in, which `AppKit` keeps for as
    // long as the window is open, and the window is open while it's being drawn in.
    let view: &NSView = unsafe { appkit.ns_view.cast().as_ref() };
    Some(Retained::from(view))
}

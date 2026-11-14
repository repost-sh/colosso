//! What the harness reads of the app's windows: their views, and how they stand.

use std::fmt::Write as _;

use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSApplication, NSView, NSWindow};

use super::class_name;

/// Every window of the app: its number, class, title, frame, and how it stands.
pub(super) fn windows() -> String {
    let Some(main) = MainThreadMarker::new() else {
        return String::new();
    };
    let mut text = String::new();
    for window in &NSApplication::sharedApplication(main).windows() {
        let frame = window.frame();
        // SAFETY: NSWindow's own.
        let occlusion: usize = unsafe { msg_send![&*window, occlusionState] };
        let content = (window.contentView()).map_or("none".to_owned(), |view| class_name(&view));
        let _ = write!(
            text,
            "\n{} {} “{}” {:.0},{:.0} {:.0}x{:.0} visible {} key {} seen {} alpha {} content {}",
            window.windowNumber(),
            class_name(&window),
            window.title(),
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            window.isVisible(),
            window.isKeyWindow(),
            occlusion & 2 != 0,
            window.alphaValue(),
            content,
        );
    }
    text
}

pub(super) fn state(window: &NSWindow) -> String {
    // SAFETY: NSWindow's own.
    let responder: *mut AnyObject = unsafe { msg_send![window, firstResponder] };
    // SAFETY: an object, or nil.
    let responder = unsafe { responder.as_ref() }.map_or("none".to_owned(), class_name);
    // SAFETY: NSWindow's own.
    let occlusion: usize = unsafe { msg_send![window, occlusionState] };
    let active = MainThreadMarker::new()
        .is_some_and(|main| NSApplication::sharedApplication(main).isActive());
    format!(
        "first responder {responder}, key {}, main {}, seen {}, app active {active}, number {}",
        window.isKeyWindow(),
        window.isMainWindow(),
        occlusion & 2 != 0,
        window.windowNumber()
    )
}

pub(super) fn views(view: &NSView, depth: usize, text: &mut String) {
    let frame = view.frame();
    let _ = write!(
        text,
        "\n{}{} {:.0},{:.0} {:.0}x{:.0}{}",
        "  ".repeat(depth),
        class_name(view),
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
        if view.isHidden() { " hidden" } else { "" }
    );
    for child in &view.subviews() {
        views(&child, depth + 1, text);
    }
}

//! What the harness reads of the app's windows: their views, and how they stand.

use std::fmt::Write as _;

use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, class, msg_send, sel};
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

/// The pointer's cursor as the app has it set, and where the window is on screen (from the
/// screen's top left), for a real pointer to be put over a spot of it.
pub(super) fn cursor(window: &NSWindow) -> String {
    let names = [
        ("arrow", sel!(arrowCursor)),
        ("hand", sel!(pointingHandCursor)),
        ("text", sel!(IBeamCursor)),
        ("crosshair", sel!(crosshairCursor)),
        ("resize-left-right", sel!(resizeLeftRightCursor)),
        ("resize-up-down", sel!(resizeUpDownCursor)),
    ];
    // SAFETY: NSCursor's own class methods, each answering a shared cursor, compared by
    // identity.
    let name = unsafe {
        let current: *mut AnyObject = msg_send![class!(NSCursor), currentCursor];
        names
            .iter()
            .find(|(_, selector)| {
                let known: *mut AnyObject = msg_send![class!(NSCursor), performSelector: *selector];
                known == current
            })
            .map_or("other", |(name, _)| name)
    };
    let frame = window.frame();
    let screen = window.screen().map_or(0., |screen| screen.frame().size.height);
    format!(
        "{name}; window at {},{} of the screen",
        frame.origin.x,
        screen - frame.origin.y - frame.size.height
    )
}

/// Whether windows nothing shows are drawn all the same (`tick`).
pub(super) static TICKS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Has GPUI lay out and paint every window of the app that nothing shows, which it wouldn't
/// by itself: on a desktop that isn't the one shown, the app goes on as if it were seen.
pub(super) fn draw_unseen() {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    for window in &NSApplication::sharedApplication(main).windows() {
        // SAFETY: NSWindow's own.
        let occlusion: usize = unsafe { msg_send![&*window, occlusionState] };
        if occlusion & 2 != 0 || !window.isVisible() {
            continue;
        }
        let layer = (window.contentView())
            .and_then(|content| koyo_browser_host::native::find_view(&content, c"GPUIView"))
            .and_then(|view| view.layer());
        if let Some(layer) = layer {
            layer.display();
        }
    }
}

/// Hands a pointer's move to the view under it and to GPUI's view, whose tracking areas both
/// get it from `AppKit` where the pointer is really over the window; what the view was.
pub(super) fn hover(window: &NSWindow, event: *mut AnyObject) -> String {
    let Some(content) = window.contentView() else {
        return "no content".into();
    };
    if event.is_null() {
        return "no event".into();
    }
    // SAFETY: NSEvent's and NSView's own; the event is alive through the calls.
    unsafe {
        let at: objc2_foundation::NSPoint = msg_send![event, locationInWindow];
        let hit: *mut AnyObject = msg_send![&*content, hitTest: at];
        let gpui = koyo_browser_host::native::find_view(&content, c"GPUIView");
        if let Some(gpui) = &gpui {
            let _: () = msg_send![&**gpui, mouseMoved: event];
        }
        let Some(hit) = hit.cast::<NSView>().as_ref() else {
            return "over nothing".into();
        };
        if gpui.as_deref().is_none_or(|gpui| !std::ptr::eq(gpui, hit)) {
            let _: () = msg_send![hit, mouseMoved: event];
        }
        format!("over {}", class_name(hit))
    }
}

/// Has GPUI lay out and paint though nothing shows the window, as with the display asleep:
/// what a frame does (a page placed, the keyboard moved) is done.
pub(super) fn draw(window: &NSWindow) -> String {
    let layer = (window.contentView())
        .and_then(|content| koyo_browser_host::native::find_view(&content, c"GPUIView"))
        .and_then(|view| view.layer());
    match layer {
        Some(layer) => {
            layer.display();
            "drawn".into()
        }
        None => "no GPUI view".into(),
    }
}

/// Quits the app, as its menu's Quit does.
pub(super) fn quit() -> String {
    if let Some(main) = MainThreadMarker::new() {
        // SAFETY: `NSApplication`'s own, as the Quit menu item sends it.
        unsafe {
            let _: () = msg_send![
                &*NSApplication::sharedApplication(main),
                terminate: std::ptr::null::<AnyObject>()
            ];
        }
    }
    "quitting".into()
}

/// The layers under the window's native pages, by class: frame, whether hidden, opacity, what
/// they hold. A page drawn by software holds its picture in these, in the app's process.
pub(super) fn layers(window: &NSWindow) -> String {
    let mut text = String::new();
    let gpui = (window.contentView())
        .and_then(|content| koyo_browser_host::native::find_view(&content, c"GPUIView"));
    if let Some(layer) = gpui.and_then(|view| view.layer()) {
        // SAFETY: a layer of the window's, walked on the main thread.
        unsafe { layer_tree(&*std::ptr::from_ref(&*layer).cast::<AnyObject>(), 0, &mut text) };
    }
    text
}

/// One layer and those under it.
///
/// # Safety
/// `layer` is a `CALayer`, on the main thread.
unsafe fn layer_tree(layer: &AnyObject, depth: usize, text: &mut String) {
    // SAFETY: CALayer's own, as the caller says.
    unsafe {
        let frame: objc2_foundation::NSRect = msg_send![layer, frame];
        let hidden: bool = msg_send![layer, isHidden];
        let opacity: f32 = msg_send![layer, opacity];
        let contents: *mut AnyObject = msg_send![layer, contents];
        let background: *mut std::ffi::c_void = msg_send![layer, backgroundColor];
        let delegate: *mut AnyObject = msg_send![layer, delegate];
        let _ = write!(
            text,
            "\n{}{} {:.0},{:.0} {:.0}x{:.0}{}{}{}{}{}",
            "  ".repeat(depth),
            class_name(layer),
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            if hidden { " hidden" } else { "" },
            if opacity < 1. { format!(" opacity {opacity}") } else { String::new() },
            contents.as_ref().map_or(String::new(), |held| format!(" holds {}", class_name(held))),
            if background.is_null() { "" } else { " coloured" },
            delegate.as_ref().map_or(String::new(), |of| format!(" of {}", class_name(of))),
        );
        if depth > 12 {
            return;
        }
        let under: *mut AnyObject = msg_send![layer, sublayers];
        let Some(under) = under.as_ref() else {
            return;
        };
        let count: usize = msg_send![under, count];
        for at in 0..count {
            let sub: *mut AnyObject = msg_send![under, objectAtIndex: at];
            if let Some(sub) = sub.as_ref() {
                layer_tree(sub, depth + 1, text);
            }
        }
    }
}

/// The app's window of that number, as `windows` lists them.
pub(super) fn numbered(number: isize) -> Option<objc2::rc::Retained<NSWindow>> {
    let main = MainThreadMarker::new()?;
    let windows = NSApplication::sharedApplication(main).windows();
    windows.iter().find(|window| window.windowNumber() == number)
}

/// Evaluates a script in a page, by its pane's number as `pages` lists them: `<pane> <script>`.
/// Any page of the host's, the developer tools' own too, which no browser tool reaches.
pub(super) async fn eval(rest: &str, cx: &mut gpui_kit::AsyncApp) -> String {
    let (pane, script) = rest.split_once(' ').unwrap_or((rest, ""));
    let Ok(pane) = pane.parse() else {
        return "no pane".into();
    };
    let params = serde_json::json!({ "expression": script, "returnByValue": true });
    let answered = cx.update(|cx| {
        let browsers = crate::browser::host::Browsers::global(cx);
        browsers.update(cx, |browsers, _| browsers.call(pane, "Runtime.evaluate", params))
    });
    match answered.await {
        Ok(Ok(answer)) => answer.to_string(),
        Ok(Err(why)) => why,
        Err(_) => "no answer".into(),
    }
}

/// A left drag across the window, as a hand makes it: a press at the first point, a move to
/// each of the others a pause apart, so what answers a move has its turn before the next, and
/// a release at the last. `<ms a move> <x> <y> <x> <y> …`, from the window's top left.
pub(super) async fn drag(rest: &str, cx: &mut gpui_kit::AsyncApp) -> String {
    const USAGE: &str = "drag <ms> <x> <y> <x> <y> …";
    let given: Vec<f64> = rest.split(' ').filter_map(|word| word.parse().ok()).collect();
    let Some((pause, points)) = given.split_first() else {
        return USAGE.into();
    };
    let ([first, moves @ ..], _) = points.as_chunks::<2>() else {
        return USAGE.into();
    };
    let Some(last) = moves.last() else {
        return USAGE.into();
    };
    let Some(window) = super::window() else {
        return "no window".into();
    };
    let pause = std::time::Duration::from_millis(*pause as u64);
    let at = |[x, y]: &[f64; 2]| super::point(&window, *x, *y);
    // The pointer moved there (5), a left press (1), a left drag to each point (6), and the
    // release (2).
    super::mouse(&window, 5, at(first), 0);
    super::mouse(&window, 1, at(first), 1);
    for to in moves {
        super::mouse(&window, 6, at(to), 1);
        cx.background_executor().timer(pause).await;
    }
    super::mouse(&window, 2, at(last), 1);
    "dragged".into()
}

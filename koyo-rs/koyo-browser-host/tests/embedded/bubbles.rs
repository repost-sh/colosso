//! A window Chromium shows for a page and ties to its holder (a bubble, an autofill list, an
//! extension's popup) goes where the page's views go (`native/holder.rs`): into the app's
//! window the page is drawn in, back to the holder as the page leaves it, out again as it comes
//! back, and taken down wherever it is once Chromium takes it from the holder.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use koyo_browser_host::{embedded, native};
use koyo_browser_ipc::AppRequest;
use koyo_browser_ipc::client::{Connection, Incoming};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSApplication, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::inside::open;

/// Far off every screen, where the test's windows are shown.
const AWAY: NSPoint = NSPoint::new(-20000., -20000.);

/// The page leaves the app's window with a bubble of Chromium's up, and comes back: Chromium
/// then taking the bubble from the holder takes it from the app's window.
pub fn leave_and_return(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    host.send(&AppRequest::Sight { pane: 1, kept: true, seen: true })
    .map_err(|error| format!("visible: {error}"))?;
    let (done, said) = mpsc::channel();
    embedded::on_main(move || {
        let _ = done.send(on_main());
    });
    said.recv_timeout(Duration::from_secs(10))
        .map_err(|error| format!("the main thread didn't move the page: {error}"))?
}

/// The cycle, on the main thread.
fn on_main() -> Result<(), String> {
    let main = MainThreadMarker::new().ok_or("not on the main thread")?;
    let holder = holder(main).ok_or("no holder holds the page")?;
    let window = made(main, NSSize::new(400., 300.));
    window.orderBack(None);
    let bubble = made(main, NSSize::new(100., 50.));
    let content = window.contentView().ok_or("the app's window has no view")?;
    let screen = window.frame();
    native::place(1, &content, &content, screen);
    let outcome = cycle(&holder, &window, &bubble, &content, screen);
    native::release(1, &content);
    bubble.orderOut(None);
    window.orderOut(None);
    outcome
}

/// Shows `bubble` as Chromium does, moves the page out of `window` and back, and takes the
/// bubble down as Chromium does: where the bubble is at each step.
fn cycle(
    holder: &NSWindow,
    window: &NSWindow,
    bubble: &NSWindow,
    content: &objc2_app_kit::NSView,
    screen: NSRect,
) -> Result<(), String> {
    // SAFETY: two live windows, the second added to the first as `AppKit` declares
    // (`NSWindowAbove`), on the main thread, as Chromium adds its bubbles.
    unsafe {
        let _: () = msg_send![holder, addChildWindow: bubble, ordered: 1_isize];
    }
    parent_is(bubble, Some(window), "shown, the bubble is in front of the app's window")?;
    native::release(1, content);
    parent_is(bubble, Some(holder), "the page gone from the app's window, it's the holder's")?;
    native::place(1, content, content, screen);
    parent_is(bubble, Some(window), "the page back, it's in front of the app's window again")?;
    holder.removeChildWindow(bubble);
    parent_is(bubble, None, "Chromium taking it from the holder takes it from the app's window")?;
    eprintln!("a bubble went with its page out of the app's window and back, and was taken down");
    Ok(())
}

/// The window whose views are the page's: its holder.
fn holder(main: MainThreadMarker) -> Option<Retained<NSWindow>> {
    let windows = NSApplication::sharedApplication(main).windows();
    windows.iter().find(|window| {
        window
            .contentView()
            .and_then(|content| native::find_view(&content, c"WebContentsViewCocoa"))
            .is_some()
    })
}

/// A borderless window of `size`, off every screen.
pub fn made(main: MainThreadMarker, size: NSSize) -> Retained<NSWindow> {
    let frame = NSRect::new(AWAY, size);
    // SAFETY: `NSWindow`'s initializer, with a borderless style, a buffered backing store and
    // no deferring, on the main thread.
    let window: Retained<NSWindow> = unsafe {
        msg_send![main.alloc::<NSWindow>(), initWithContentRect: frame, styleMask: 0_usize, backing: 2_usize, defer: Bool::NO]
    };
    // SAFETY: the window is kept by the test until it's dropped, never released as it closes.
    unsafe { window.setReleasedWhenClosed(false) };
    window
}

/// Whether `child`'s parent is `parent`, as `what` says it should be.
fn parent_is(child: &NSWindow, parent: Option<&NSWindow>, what: &str) -> Result<(), String> {
    let now = child.parentWindow();
    let address = |window: Option<&NSWindow>| window.map_or(0, |window| std::ptr::from_ref(window) as usize);
    if address(now.as_deref()) == address(parent) {
        return Ok(());
    }
    let named = |window: Option<&NSWindow>| match window {
        None => "none".to_owned(),
        Some(window) => format!("{:#x}", address(Some(window))),
    };
    Err(format!(
        "{what}: its parent is {}, not {}",
        named(now.as_deref()),
        named(parent)
    ))
}

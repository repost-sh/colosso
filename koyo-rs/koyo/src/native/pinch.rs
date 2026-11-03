//! Trackpad pinches over a windowless surface's view. GPUI's window hands a native surface
//! the pointer's presses, moves and scrolls, but not its pinches: `AppKit` gives those to the
//! view under the pointer, the surface's container, which does nothing with them. So the app
//! looks at each pinch before `AppKit` hands it out, and takes the ones over the container.
//!
//! This stands in for gpui-fast routing pinches as it does scrolls (gpui-fast-macos's
//! `send_window_event`, gpui-fast's `event_target` and `dispatch_native`), and reads the event
//! as gpui-fast-macos's `platform_input_from_native` does, but for a cancelled pinch, which
//! stays cancelled so its fingers lift. Once gpui-fast routes pinches, this goes.

use std::ptr::{self, NonNull};

use block2::RcBlock;
use gpui_kit::{Modifiers, PinchEvent, PlatformInput, TouchPhase, point, px};
use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventPhase, NSView};

use super::surface::Pointer;

/// Pinches over a view going to its pointer, for as long as this is kept.
pub struct Pinches(Retained<AnyObject>);

impl Drop for Pinches {
    fn drop(&mut self) {
        // SAFETY: the monitor `addLocalMonitorForEventsMatchingMask:handler:` returned, removed
        // once.
        unsafe { NSEvent::removeMonitor(&self.0) };
    }
}

/// Hands `pointer` each pinch over `container` from now on, in the window's logical pixels.
pub fn watch(container: &NSView, pointer: Pointer) -> Option<Pinches> {
    let container = Weak::from(container);
    let pinched = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit hands the monitor an event that lives for the call.
        let native = unsafe { event.as_ref() };
        match container
            .load()
            .and_then(|container| pinch_over(&container, native))
        {
            Some(pinch) => {
                pointer(&pinch);
                // Nothing else under the pointer would take it.
                ptr::null_mut()
            }
            None => event.as_ptr(),
        }
    });
    // SAFETY: the block returns the event it's given or none, as a monitor's must, and runs on
    // the main thread with the events it watches.
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::Magnify, &pinched)
    };
    monitor.map(Pinches)
}

/// The pinch `event` is, when it's over `container`: what `AppKit`'s hit testing finds under
/// it is in `container`, which no GPUI overlay covers there.
fn pinch_over(container: &NSView, event: &NSEvent) -> Option<PlatformInput> {
    let window = event.window(MainThreadMarker::new()?)?;
    let content = window.contentView()?;
    let location = event.locationInWindow();
    // SAFETY: on the main thread, of the content view of the window the event is in, which
    // AppKit keeps for as long as the window is shown.
    let frame = unsafe { content.superview() };
    let at = frame.map_or(location, |frame| {
        frame.convertPoint_fromView(location, None)
    });
    if !content.hitTest(at)?.isDescendantOf(container) {
        return None;
    }
    let phase = match event.phase() {
        NSEventPhase::MayBegin | NSEventPhase::Began => TouchPhase::Started,
        NSEventPhase::Ended => TouchPhase::Ended,
        NSEventPhase::Cancelled => TouchPhase::Cancelled,
        _ => TouchPhase::Moved,
    };
    let flags = event.modifierFlags();
    let height = content.frame().size.height;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "points and a magnification fit an f32"
    )]
    Some(PlatformInput::Pinch(PinchEvent {
        position: point(px(location.x as f32), px((height - location.y) as f32)),
        delta: event.magnification() as f32,
        modifiers: Modifiers {
            control: flags.contains(NSEventModifierFlags::Control),
            alt: flags.contains(NSEventModifierFlags::Option),
            shift: flags.contains(NSEventModifierFlags::Shift),
            platform: flags.contains(NSEventModifierFlags::Command),
            function: flags.contains(NSEventModifierFlags::Function),
        },
        phase,
    }))
}

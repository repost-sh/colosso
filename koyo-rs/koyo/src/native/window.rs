//! A GPUI window's `NSWindow`, for what GPUI doesn't do with it: fitting a window to its content
//! from the corner it keeps, where GPUI's resize keeps the bottom left, as `AppKit` does; keeping
//! a floating window to its Space; showing one that opened unseen; and its traffic lights
//! (`traffic_lights.rs`).

use gpui_kit::{AnyWindowHandle, App, AsyncApp, Pixels, Size, Window};
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSViewLayerContentsPlacement, NSWindow, NSWindowCollectionBehavior};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::popup::Corner;

/// Makes `window`'s content `extent`, `corner` staying where it is; false when it has no
/// `NSWindow`, as a test's window hasn't. The frame changes once the app is free of the update
/// asking for it: `AppKit` tells GPUI of a resize as it happens, which can't be heard mid-update.
///
/// The frame GPUI drew last is the old size. Left to itself, Core Animation stretches it to the
/// new one until GPUI's next frame, which comes late while the app is busy: a launcher whose rows
/// come and go showed its field squashed or stretched. So the frame is pinned to the kept corner
/// rather than stretched, which a view laid out from that corner shows right, and drawn again at
/// the new size before the resize reaches the screen, in the same Core Animation transaction.
pub fn fit(window: &Window, extent: Size<Pixels>, corner: Corner, cx: &mut App) -> bool {
    let Some(view) = gpui_view(window) else {
        return false;
    };
    let Some(ns_window) = view.window() else {
        return false;
    };
    view.setLayerContentsPlacement(match (corner.top, corner.left) {
        (true, true) => NSViewLayerContentsPlacement::TopLeft,
        (true, false) => NSViewLayerContentsPlacement::TopRight,
        (false, true) => NSViewLayerContentsPlacement::BottomLeft,
        (false, false) => NSViewLayerContentsPlacement::BottomRight,
    });
    let (width, height) = (
        f64::from(extent.width.as_f32()),
        f64::from(extent.height.as_f32()),
    );
    cx.spawn(async move |_: &mut AsyncApp| {
        let frame = ns_window.frame();
        let content = ns_window.contentRectForFrameRect(frame);
        let mut fitted = frame;
        fitted.size.width = width + frame.size.width - content.size.width;
        fitted.size.height = height + frame.size.height - content.size.height;
        // Screen coordinates rise from the bottom: a kept top moves the bottom, a kept right
        // moves the left.
        if corner.top {
            fitted.origin.y = frame.origin.y + frame.size.height - fitted.size.height;
        }
        if !corner.left {
            fitted.origin.x = frame.origin.x + frame.size.width - fitted.size.width;
        }
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

/// Has GPUI draw `window` once the app is free of the update asking for it, rather than at the
/// screen's next refresh, presenting the frame in the Core Animation transaction a native
/// view's move commits in: what GPUI draws and the view land on screen together. For a frame
/// that moves a native view against GPUI's own content, as a pane's edge dragged along a
/// native page; a window drawn at the screen's refresh presents by itself, a refresh or two
/// apart from the view.
#[cfg(not(test))]
pub fn draw_in_transaction(window: &Window, cx: &mut App) {
    let Some(layer) = gpui_view(window).and_then(|view| view.layer()) else {
        return;
    };
    thread_local! {
        /// Whether a draw is asked for and not yet made: a drag's moves, and the pages it
        /// resizes, come faster than frames, and one draw shows them all.
        static ASKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if ASKED.replace(true) {
        return;
    }
    // GPUI's view draws its layer when asked to display it, presenting in the transaction.
    cx.spawn(async move |_: &mut AsyncApp| {
        ASKED.set(false);
        layer.display();
    })
    .detach();
}

/// Makes `window` a callout: a rounded rectangle with an arrow on its top edge, `tip` high and
/// twice `half` wide, its point `at` points along from the window's left. The window loses the
/// frame macOS gives it, whose rounded edge is drawn round the whole rectangle whatever is in
/// it, and its blur is cut to the callout's shape, so nothing but the callout is there and
/// macOS draws the shadow round that. Done once the app is free of the update asking for it:
/// `AppKit` tells GPUI of a window's changes as they happen. Nothing where the window has no
/// blur of its own, as a test's hasn't.
pub fn callout(window: &Window, at: f32, tip: f32, half: f32, radius: f32, cx: &mut App) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Some(ns_window) = ns_window(window) else {
        return;
    };
    let Some(blur) = (ns_window.contentView()).and_then(|content| effect_view(&content)) else {
        return;
    };
    let (at, tip, half, radius) = (f64::from(at), f64::from(tip), f64::from(half), f64::from(radius));
    cx.spawn(async move |_: &mut AsyncApp| {
        // SAFETY: AppKit's own, on the main thread: the window's style and colour, an image
        // its size drawn into while it has the focus, and the effect view, which keeps it.
        unsafe {
            // Titled, closable, miniaturizable, resizable, and content under the title bar:
            // what gives a window its frame. What else it is, as a panel that takes no
            // activation, stays.
            let style: usize = msg_send![&*ns_window, styleMask];
            let bare = style & !(0b1111 | (1 << 15));
            if bare != style {
                let _: () = msg_send![&*ns_window, setStyleMask: bare];
            }
            let clear: Retained<AnyObject> = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![&*ns_window, setBackgroundColor: &*clear];
            ns_window.setOpaque(false);
            let extent = ns_window.frame().size;
            // A point inside the outline the app draws, so no blur shows beyond its rim.
            let inset = 1.;
            let body = NSRect::new(
                NSPoint::new(inset, inset),
                NSSize::new(extent.width - 2. * inset, extent.height - tip - inset),
            );
            let radius = radius - inset / 2.;
            let image: Retained<AnyObject> = msg_send![msg_send![class!(NSImage), alloc], initWithSize: extent];
            let _: () = msg_send![&*image, lockFocus];
            let black: Retained<AnyObject> = msg_send![class!(NSColor), blackColor];
            let _: () = msg_send![&*black, setFill];
            let rounded: Retained<AnyObject> = msg_send![
                class!(NSBezierPath),
                bezierPathWithRoundedRect: body,
                xRadius: radius,
                yRadius: radius
            ];
            let _: () = msg_send![&*rounded, fill];
            // The image's origin is its bottom left: the arrow stands on the body's top edge.
            let arrow: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPath];
            let _: () = msg_send![&*arrow, moveToPoint: NSPoint::new(at - half + inset, extent.height - tip)];
            let _: () = msg_send![&*arrow, lineToPoint: NSPoint::new(at, extent.height - inset)];
            let _: () = msg_send![&*arrow, lineToPoint: NSPoint::new(at + half - inset, extent.height - tip)];
            let _: () = msg_send![&*arrow, closePath];
            let _: () = msg_send![&*arrow, fill];
            let _: () = msg_send![&*image, unlockFocus];
            // Stretched only between its corners, top and bottom, so the shape holds while the
            // window grows or shrinks to what it lists, until it's cut again at the new size.
            let caps = objc2_foundation::NSEdgeInsets {
                top: tip + radius + inset,
                left: 0.,
                bottom: radius + inset,
                right: 0.,
            };
            let _: () = msg_send![&*image, setCapInsets: caps];
            let _: () = msg_send![&*blur, setMaskImage: &*image];
        }
        ns_window.invalidateShadow();
    })
    .detach();
    // And the shadow again once what's drawn in it has settled.
    refresh_edge(window.window_handle(), cx);
}

/// The effect view under `view` that blurs what's behind its window, if it has one.
fn effect_view(view: &NSView) -> Option<Retained<NSView>> {
    use objc2::{class, msg_send};
    // SAFETY: NSObject's own.
    let blurs: bool = unsafe { msg_send![view, isKindOfClass: class!(NSVisualEffectView)] };
    if blurs {
        return Some(Retained::from(view));
    }
    view.subviews().iter().find_map(|under| effect_view(&under))
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

/// Shows `window`, opened unseen, over the others, without giving it the keyboard.
pub fn show(window: &Window) {
    if let Some(ns_window) = ns_window(window) {
        ns_window.orderFront(None);
    }
}

/// Keeps a floating `window` to the Space it opens on, and unseen while Koyo isn't the app in
/// front, coming back with it. GPUI has a pop-up panel join every Space (and full-screen ones), so
/// a menu left open went with the person to another desktop, and a dialog floated over the app
/// they went to. It may still show over a full-screen window of Koyo's.
pub fn float_on_its_space(window: &Window) {
    if let Some(ns_window) = ns_window(window) {
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::Managed | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        ns_window.setHidesOnDeactivate(true);
    }
}

/// The `NSWindow` GPUI draws `window` in; none for a test's window.
pub(crate) fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    gpui_view(window)?.window()
}

/// The view GPUI draws `window` in, its layer GPUI's; none for a test's window.
pub(crate) fn gpui_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: an `AppKit` handle's view is the view GPUI draws in, which `AppKit` keeps for as
    // long as the window is open, and the window is open while it's being drawn in.
    let view: &NSView = unsafe { appkit.ns_view.cast().as_ref() };
    Some(Retained::from(view))
}

//! A GPUI window's `NSWindow`, for what GPUI doesn't do with it: fitting a window to its content
//! from the corner it keeps, where GPUI's resize keeps the bottom left, as `AppKit` does; keeping
//! a floating window to its Space; showing one that opened unseen; and its traffic lights
//! (`traffic_lights.rs`).

use gpui_kit::{AnyWindowHandle, App, AsyncApp, Pixels, Size, Window};
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSViewLayerContentsPlacement, NSWindow};
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
        if corner.top {
            fitted.origin.y = frame.origin.y + frame.size.height - fitted.size.height;
        }
        if !corner.left {
            fitted.origin.x = frame.origin.x + frame.size.width - fitted.size.width;
        }
        ns_window.setFrame_display(fitted, true);
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
    thread_local! {
        /// Whether a draw is asked for and not yet made: a drag's moves, and the pages it
        /// resizes, come faster than frames, and one draw shows them all.
        static ASKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if ASKED.get() {
        return;
    }
    let Some(layer) = gpui_view(window).and_then(|view| view.layer()) else {
        return;
    };
    ASKED.set(true);
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
    let Some(blur) = blur_view(window) else {
        return;
    };
    let (at, tip, half, radius) = (f64::from(at), f64::from(tip), f64::from(half), f64::from(radius));
    cx.spawn(async move |_: &mut AsyncApp| {
        // SAFETY: AppKit's own, on the main thread: the window's style and colour, an image
        // its size drawn into while it has the focus, and the effect view, which keeps it.
        unsafe {
            let style: usize = msg_send![&*ns_window, styleMask];
            let bare = style & !(0b1111 | (1 << 15));
            if bare != style {
                let _: () = msg_send![&*ns_window, setStyleMask: bare];
            }
            let clear: Retained<AnyObject> = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![&*ns_window, setBackgroundColor: &*clear];
            ns_window.setOpaque(false);
            let extent = ns_window.frame().size;
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
            let arrow: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPath];
            let _: () = msg_send![&*arrow, moveToPoint: NSPoint::new(at - half + inset, extent.height - tip)];
            let _: () = msg_send![&*arrow, lineToPoint: NSPoint::new(at, extent.height - inset)];
            let _: () = msg_send![&*arrow, lineToPoint: NSPoint::new(at + half - inset, extent.height - tip)];
            let _: () = msg_send![&*arrow, closePath];
            let _: () = msg_send![&*arrow, fill];
            let _: () = msg_send![&*image, unlockFocus];
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
    refresh_edge(window.window_handle(), cx);
}

/// Has `window`'s glass blur `scale` times as strongly as its material does, 0 not at all, by
/// way of the blur view GPUI puts under a blurred window's content (`setKoyoBlurScale:`, in
/// `vendor/gpui-fast-macos`), which keeps it through `AppKit`'s updates.
pub fn blur(window: &Window, scale: f64) {
    use objc2::msg_send;

    if let Some(view) = blur_view(window) {
        // SAFETY: a `BlurredView`, the class GPUI's macOS windows blur with, which answers
        // `setKoyoBlurScale:`, on the main thread the window is drawn on.
        let _: () = unsafe { msg_send![&*view, setKoyoBlurScale: scale] };
    }
}

/// The `BlurredView` GPUI puts under a blurred window's content, if `window` has one.
fn blur_view(window: &Window) -> Option<Retained<NSView>> {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;

    let class = AnyClass::get(c"BlurredView")?;
    let content = ns_window(window)?.contentView()?;
    content.subviews().iter().find(|view| {
        // SAFETY: NSObject's own.
        unsafe { msg_send![&**view, isKindOfClass: class] }
    })
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

/// Makes `window` one of Koyo's floating windows, as the native layer has them
/// (`koyo_seam::windows::float`): a level under the menus', kept to the Space it opens on, and
/// unseen while Koyo isn't the app in front.
pub fn float_on_its_space(window: &Window) {
    if let Some(ns_window) = ns_window(window) {
        // SAFETY: a live window, on the main thread, where GPUI's windows are.
        unsafe { koyo_seam::windows::float(Retained::as_ptr(&ns_window).cast_mut().cast()) };
    }
}

/// Takes `window`, a floating window of Koyo's that's closing, off the screen now
/// (`koyo_seam::windows::dismiss`).
pub fn dismiss(window: &NSWindow) {
    // SAFETY: a live window, on the main thread.
    unsafe { koyo_seam::windows::dismiss(std::ptr::from_ref(window).cast_mut().cast()) };
}

/// Where the point `at` of `window`, counted from its top left, is on the screen: in points
/// from the main screen's bottom left, as `AppKit` counts them. None for a test's window.
pub fn on_screen(window: &Window, at: gpui_kit::Point<Pixels>) -> Option<[f64; 2]> {
    let view = gpui_view(window)?;
    let ns_window = view.window()?;
    let points = |pixels: Pixels| f64::from(f32::from(pixels));
    let in_view = objc2_foundation::NSPoint::new(points(at.x), view.frame().size.height - points(at.y));
    let on_screen = ns_window.convertPointToScreen(view.convertPoint_toView(in_view, None));
    Some([on_screen.x, on_screen.y])
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

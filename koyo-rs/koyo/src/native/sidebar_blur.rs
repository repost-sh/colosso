//! The blur behind the sidebar. macOS blurs a window with one `NSVisualEffectView`, at one
//! strength: the `BlurredView` GPUI puts under the view it draws in (`vendor/gpui-fast-macos`),
//! which keeps its material's radius. While Settings → Appearance → Sidebar blur is off that,
//! macOS's own, the sidebar has one of its own, of the same class, between that one and GPUI's
//! view and over the sidebar's surface alone, blurring the desktop behind the sidebar as strongly
//! as Sidebar blur says (`setKoyoBlurScale:`). At macOS's own it has none: the window's blurs it
//! as it blurs the rest (`Settings::sidebar_blur_scale`).
//!
//! It's put where the surface is as that's prepainted, as far as what holds it shows it, as while
//! ⌘B slides the sidebar, and rounded at the corners it shares with the card it's in, Orca's; a
//! frame that moves nothing tells `AppKit` nothing. Between frames `AppKit` keeps it to the
//! window's height, as the sidebar is, between the rails above and below it. It's taken out of
//! the window the frame it isn't drawn: the sidebar folded away, a page shown in full screen, or
//! Sidebar blur back at macOS's own. A window with no blur of its own, as a test's, has none.

use std::cell::Cell;

use gpui_kit::{
    AnyElement, App, Bounds, Corners, ElementId, IntoElement as _, Pixels, Styled as _, Window,
    canvas,
};
use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyClass;
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::surface::{self, Hosted};
use crate::theme::{self, card};

/// What fills the sidebar's surface to put the blur behind it, blurring `scale` times as strongly
/// as the window's material does, 0 not at all (`Settings::sidebar_blur_scale`).
pub fn behind(scale: f64) -> AnyElement {
    canvas(move |bounds, window, cx| place(bounds, scale, window, cx), |_, (), _, _| {})
        .size_full()
        .into_any_element()
}

/// Puts the blur behind the surface at `bounds`, as far as what holds the surface shows it.
fn place(bounds: Bounds<Pixels>, scale: f64, window: &mut Window, cx: &mut App) {
    let kept = surface::keep(ElementId::Name("sidebar-blur".into()), window, cx, Blur::new);
    let Some(blur) = &kept.read(cx).0 else {
        return;
    };
    let shown = bounds.intersect(&window.content_mask().bounds);
    let corners = card::prepainted()
        .map_or_else(Corners::default, |island| card::shared_corners(shown, island));
    blur.put(shown, corners, scale, window.viewport_size().height);
}

/// The sidebar's `BlurredView`, and where and how it was last put.
struct Blur {
    view: Retained<NSView>,
    /// Its frame in the window's content view, measured from the bottom.
    frame: Cell<Option<NSRect>>,
    /// Which of its corners are rounded as the card's.
    rounded: Cell<Option<Corners<bool>>>,
    /// How many times as strongly as its material it blurs.
    scale: Cell<Option<f64>>,
}

impl Hosted for Blur {
    fn remove(self, _: &mut App) {
        self.view.removeFromSuperview();
    }
}

impl Blur {
    /// A `BlurredView` in `window`'s content view, over the window's own and under GPUI's view;
    /// none where the window has no blur of its own.
    fn new(window: &Window) -> Option<Blur> {
        let own = super::window::blur_view(window)?;
        let content = super::window::ns_window(window)?.contentView()?;
        let class = AnyClass::get(c"BlurredView")?;
        // SAFETY: `BlurredView` is GPUI's subclass of `NSVisualEffectView`, allocated and
        // initialised as `AppKit` makes any view, on the main thread the window is drawn on; its
        // `initWithFrame:` has it blur what's behind the window, as the window's own does.
        let view: Retained<NSView> = unsafe {
            let made: Allocated<NSView> = msg_send![class, alloc];
            msg_send![made, initWithFrame: NSRect::ZERO]
        };
        view.setWantsLayer(true);
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
        content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, Some(&own));
        Some(Blur {
            view,
            frame: Cell::new(None),
            rounded: Cell::new(None),
            scale: Cell::new(None),
        })
    }

    /// Puts the blur over `shown`, measured from the top left of the window's content, `height`
    /// tall, rounded as the card is at `corners`, blurring `scale` times as strongly as its
    /// material does.
    fn put(&self, shown: Bounds<Pixels>, corners: Corners<bool>, scale: f64, height: Pixels) {
        if self.scale.replace(Some(scale)) != Some(scale) {
            // SAFETY: a `BlurredView`, which answers `setKoyoBlurScale:`, on the main thread the
            // window is drawn on.
            let _: () = unsafe { msg_send![&*self.view, setKoyoBlurScale: scale] };
        }
        let points = |pixels: Pixels| f64::from(f32::from(pixels));
        let frame = if shown.is_empty() {
            NSRect::ZERO
        } else {
            NSRect::new(
                NSPoint::new(points(shown.left()), points(height) - points(shown.bottom())),
                NSSize::new(points(shown.size.width), points(shown.size.height)),
            )
        };
        if self.frame.replace(Some(frame)) != Some(frame) {
            self.view.setFrame(frame);
        }
        if self.rounded.replace(Some(corners)) != Some(corners)
            && let Some(layer) = self.view.layer()
        {
            surface::round(&layer, corners, f64::from(theme::CARD_RADIUS));
        }
    }
}

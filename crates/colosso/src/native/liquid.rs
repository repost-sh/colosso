//! Liquid Glass behind an element GPUI draws: an `NSGlassEffectView` in a native surface of
//! the window's composition, which stacks it over the page and under the window's overlays,
//! so the element's content, drawn among them, shows over the glass. It's rounded as the element is, and light or dark as
//! the app's theme is, whatever's behind it, so the theme's text reads on it over a page of
//! any colour: left to itself, the glass would go light over a light page and dark over a
//! dark one. In a window that can't host a native view, as a test's can't, or before
//! macOS 26, there's none, and the element keeps a fill of its own.
//!
//! It moves only when the element does: a frame that moves nothing tells `AppKit` nothing.

use std::cell::Cell;
use std::sync::Once;

use gpui_kit::{
    AnyElement, App, Bounds, Context, ElementId, IntoElement as _, Pixels, Styled as _, Window,
    canvas,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, NSObjectProtocol as _};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization as _, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSGlassEffectView,
};

use super::surface::Surface;

/// Glass to put behind the element it's a child of, filling it, its corners `radius` round
/// (at most half its height) and dark or light as `dark` says; none where there's no glass.
/// `id` keys it: it's kept as long as it's drawn, made the frame it first shows and taken
/// out of the window the frame it doesn't.
pub fn backdrop(
    id: impl Into<ElementId>,
    radius: Pixels,
    dark: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let pane = window.use_keyed_state(id.into(), cx, Pane::new);
    pane.read(cx).native.as_ref()?;
    // Placed as it's painted, not prepainted: GPUI stacks a native surface on the side of
    // its overlays its element is prepainted on (gpui-fast's `begin_base` ends the prepaint
    // that tells), so placed from a deferred element's prepaint the glass would cover what's
    // drawn on it. It starts below the overlays and stays there.
    let glass = canvas(
        |_, _, _| (),
        move |bounds, (), window, cx| {
            let clip = window.content_mask().bounds;
            pane.update(cx, |pane, _| {
                if let Some(native) = &pane.native {
                    native.place(bounds, clip, radius, dark, window);
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    Some(glass.into_any_element())
}

struct Pane {
    native: Option<Native>,
}

struct Native {
    surface: Surface,
    view: Retained<NSGlassEffectView>,
    /// The corners and the appearance the glass was last given.
    corners: Cell<Option<f64>>,
    dark: Cell<Option<bool>>,
}

impl Pane {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let native = Native::new(window)
            .inspect_err(|why| {
                static SAID: Once = Once::new();
                SAID.call_once(|| log::info!("no Liquid Glass behind the app's bars: {why}"));
            })
            .ok();
        cx.on_release(|pane: &mut Self, cx| {
            if let Some(native) = pane.native.take() {
                native.surface.remove(cx);
            }
        })
        .detach();
        Pane { native }
    }
}

impl Native {
    fn new(window: &Window) -> Result<Native, String> {
        let main = MainThreadMarker::new().ok_or("not on the main thread")?;
        AnyClass::get(c"NSGlassEffectView").ok_or("the system has no Liquid Glass")?;
        let view = NSGlassEffectView::new(main);
        // Its look from its appearance, which `place` sets, not from what's behind it.
        if view.respondsToSelector(sel!(set_adaptiveAppearance:)) {
            // SAFETY: the setter, private, takes an `NSInteger`, of which 1 has the glass
            // follow its view's appearance.
            let _: () = unsafe { msg_send![&view, set_adaptiveAppearance: 1isize] };
        }
        let surface = Surface::new(window, &view)?;
        Ok(Native {
            surface,
            view,
            corners: Cell::new(None),
            dark: Cell::new(None),
        })
    }

    /// Puts the glass over `bounds`, as much of it as `clip` shows.
    fn place(
        &self,
        bounds: Bounds<Pixels>,
        clip: Bounds<Pixels>,
        radius: Pixels,
        dark: bool,
        window: &Window,
    ) {
        let corners = f64::from(f32::from(radius.min(bounds.size.height / 2.)));
        if self.corners.replace(Some(corners)) != Some(corners) {
            self.view.setCornerRadius(corners);
        }
        if self.dark.replace(Some(dark)) != Some(dark) {
            // SAFETY: names AppKit defines.
            let name = unsafe {
                if dark {
                    NSAppearanceNameDarkAqua
                } else {
                    NSAppearanceNameAqua
                }
            };
            self.view
                .setAppearance(NSAppearance::appearanceNamed(name).as_deref());
        }
        self.surface.place(bounds, clip, window);
    }
}

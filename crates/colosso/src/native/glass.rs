//! The blur behind the editor's hover and diagnostic cards: an `NSVisualEffectView` of the
//! menu material, blurring what's in the window behind it, in a native surface the window
//! composition stacks over the page and under the card. So a card is frosted as the app's
//! menus are. In a window that can't host a native view the cards stay opaque.

use std::sync::Once;

use gpui_kit::component::input::{PopoverBackdrop, set_popover_backdrop};
use gpui_kit::{App, Bounds, Context, ElementId, Entity, Pixels, Window};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
};

use super::surface::Surface;

pub fn init(cx: &mut App) {
    set_popover_backdrop(Glass, cx);
}

struct Glass;

impl PopoverBackdrop for Glass {
    fn shows(&self, window: &mut Window, cx: &mut App) -> bool {
        pane(window, cx).read(cx).native.is_some()
    }

    fn place(&self, bounds: Bounds<Pixels>, radius: Pixels, window: &mut Window, cx: &mut App) {
        pane(window, cx).update(cx, |pane, _| {
            let Some(native) = &pane.native else {
                return;
            };
            if pane.radius.replace(radius) != Some(radius) {
                native.round(radius);
            }
            native.surface.place(bounds, bounds, window);
        });
    }
}

/// The glass behind the card being drawn, kept as long as the card is: made the frame it
/// first shows, and taken out of the window the frame it doesn't.
fn pane(window: &mut Window, cx: &mut App) -> Entity<Pane> {
    window.use_keyed_state(ElementId::Name("popover-glass".into()), cx, Pane::new)
}

struct Pane {
    native: Option<Native>,
    /// The corners the glass was last given.
    radius: Option<Pixels>,
}

struct Native {
    surface: Surface,
    view: Retained<NSVisualEffectView>,
}

impl Pane {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let native = Native::new(window)
            .inspect_err(|why| {
                static SAID: Once = Once::new();
                SAID.call_once(|| log::info!("the editor's cards stay opaque: {why}"));
            })
            .ok();
        cx.on_release(|pane: &mut Self, cx| {
            if let Some(native) = pane.native.take() {
                native.surface.remove(cx);
            }
        })
        .detach();
        Pane {
            native,
            radius: None,
        }
    }
}

impl Native {
    fn new(window: &Window) -> Result<Native, String> {
        let main = MainThreadMarker::new().ok_or("not on the main thread")?;
        let view = NSVisualEffectView::new(main);
        view.setMaterial(NSVisualEffectMaterial::Menu);
        view.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        view.setState(NSVisualEffectState::Active);
        view.setWantsLayer(true);
        let surface = Surface::new(window, &view)?;
        Ok(Native { surface, view })
    }

    /// Rounds the blur's corners to the card's.
    fn round(&self, radius: Pixels) {
        // SAFETY: a view that wants a layer has one, a `CALayer`, which takes these.
        unsafe {
            let layer: *mut AnyObject = msg_send![&*self.view, layer];
            if let Some(layer) = layer.as_ref() {
                let _: () = msg_send![layer, setCornerRadius: f64::from(f32::from(radius))];
                let _: () = msg_send![layer, setMasksToBounds: Bool::YES];
            }
        }
    }
}

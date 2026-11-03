//! A slider over a few stops that `AppKit` draws: an `NSSlider` hosted in the window, so macOS
//! gives it its own thumb (Liquid Glass while it's held, on macOS 26), its tick marks and the
//! system's accent. The words under its ends stay GPUI's. Off macOS, or in a window that can't
//! host a native view (a test's), it's the GPUI [`TickedSlider`] instead, over the same stops.
//!
//! The settings keep the value itself, as for the GPUI slider: a value set by hand between
//! two stops is shown at the nearer one, and stays as it was until the thumb is moved off it.

#[cfg(target_os = "macos")]
mod appkit;
#[cfg(target_os = "macos")]
mod host;

use gpui_kit::{App, IntoElement, RenderOnce, Window};

use super::ticked_slider::TickedSlider;
use crate::settings::Settings;

/// A setting's value, picked from its stops along a track `AppKit` draws.
#[derive(IntoElement)]
pub struct NativeSlider(TickedSlider);

impl NativeSlider {
    /// The slider `id` names, over `values` from the left, showing what `get` reads from the
    /// settings and writing a stop picked with `set`.
    pub fn new(
        id: &'static str,
        values: &'static [f32],
        get: fn(&Settings) -> f32,
        set: fn(&mut Settings, f32),
    ) -> Self {
        NativeSlider(TickedSlider::new(id, values, get, set))
    }

    /// What the words under the left and the right ends say.
    pub fn ends(self, left: &'static str, right: &'static str) -> Self {
        NativeSlider(self.0.ends(left, right))
    }

    /// Greyed out by `AppKit`, and deaf to the pointer.
    pub fn disabled(self, disabled: bool) -> Self {
        NativeSlider(self.0.disabled(disabled))
    }
}

impl RenderOnce for NativeSlider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        #[cfg(target_os = "macos")]
        if let Some(slider) = host::slider(&self.0, window, cx) {
            return slider;
        }
        let _ = (window, cx);
        self.0.into_any_element()
    }
}

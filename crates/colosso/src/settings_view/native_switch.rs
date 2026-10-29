//! A setting turned on and off by a switch `AppKit` draws: an `NSSwitch` hosted in the window,
//! so macOS gives it its own knob (Liquid Glass while it's held, on macOS 26), its motion and
//! the system's accent. Off macOS, or in a window that can't host a native view (a test's),
//! it's gpui-kit's switch instead.

#[cfg(target_os = "macos")]
mod appkit;

use gpui_kit::component::switch::Switch;
use gpui_kit::{App, IntoElement, RenderOnce, Window};

use crate::settings::{self, Settings};
use crate::theme;

/// A setting on or off, as `get` reads it and `set` writes it.
#[derive(IntoElement, Clone, Copy)]
pub struct NativeSwitch {
    id: &'static str,
    get: fn(&Settings) -> bool,
    set: fn(&mut Settings, bool),
}

impl NativeSwitch {
    /// The switch `id` names, showing what `get` reads from the settings and writing a flip
    /// with `set`.
    pub fn new(id: &'static str, get: fn(&Settings) -> bool, set: fn(&mut Settings, bool)) -> Self {
        NativeSwitch { id, get, set }
    }
}

impl RenderOnce for NativeSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        #[cfg(target_os = "macos")]
        if let Some(switch) = appkit::switch(self, window, cx) {
            return switch;
        }
        let _ = window;
        let set = self.set;
        Switch::new(self.id)
            .checked((self.get)(settings::get(cx)))
            .color(theme::palette(cx).accent)
            .on_click(move |on, _, cx| settings::update(cx, |settings| set(settings, *on)))
            .into_any_element()
    }
}

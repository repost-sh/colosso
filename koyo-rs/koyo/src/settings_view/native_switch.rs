//! A setting turned on and off by a switch `AppKit` draws (`crate::toggle`): macOS's own knob
//! (Liquid Glass while it's held, on macOS 26), its motion and the system's accent; gpui-kit's
//! switch off macOS, or in a window that can't host a native view (a test's).

use gpui_kit::{App, IntoElement, RenderOnce, Window};

use crate::settings::{self, Settings};
use crate::toggle::Toggle;

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
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let set = self.set;
        Toggle::new(self.id, (self.get)(settings::get(cx)), move |on, cx| {
            settings::update(cx, |settings| set(settings, on));
        })
    }
}

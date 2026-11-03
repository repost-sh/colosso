//! The `AppKit` half of the native switch: an `NSSwitch`, and what a flip of it writes.

use gpui_kit::{AnyElement, App, IntoElement as _, ParentElement as _, Styled as _, Window, div};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSControlStateValueOff, NSControlStateValueOn, NSSwitch};

use super::NativeSwitch;
use crate::native::{self, Control};
use crate::settings::{self, Settings};

/// The switch `AppKit` draws for `switch`; none where the window can't host a native view,
/// which falls back to gpui-kit's.
pub fn switch(switch: NativeSwitch, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let on = (switch.get)(settings::get(cx));
    let (size, track) = native::host(switch.id, switch, on, window, cx)?;
    Some(div().w(size.width).child(track).into_any_element())
}

impl Control for NativeSwitch {
    type View = NSSwitch;
    type Shown = bool;

    fn make(&self, main: MainThreadMarker) -> Retained<NSSwitch> {
        NSSwitch::new(main)
    }

    /// Flips the switch only when the setting has.
    fn show(switch: &NSSwitch, on: bool) {
        if is_on(switch) != on {
            let state = if on {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            };
            switch.setState(state);
        }
    }

    fn acted(&self, switch: &NSSwitch, shown: Option<bool>, cx: &mut App) {
        take_flip(self.set, shown, is_on(switch), cx);
    }
}

fn is_on(switch: &NSSwitch) -> bool {
    switch.state() == NSControlStateValueOn
}

/// Writes a flip to `on`, `AppKit` having flipped the switch, unless the switch was last
/// `shown` so already or isn't shown yet.
fn take_flip(set: fn(&mut Settings, bool), shown: Option<bool>, on: bool, cx: &mut App) {
    if shown.is_some_and(|shown| shown != on) {
        settings::update(cx, |settings| set(settings, on));
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    const SIDEBAR: fn(&mut Settings, bool) = |settings, on| settings.sidebar_open_on_launch = on;

    fn sidebar(cx: &TestAppContext) -> bool {
        cx.read(|cx| settings::get(cx).sidebar_open_on_launch)
    }

    #[gpui_kit::test]
    fn a_flip_is_written_once_and_only_once_shown(cx: &mut TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| settings::update(cx, |settings| settings.sidebar_open_on_launch = true));
        // Not shown yet, a flip is a stray, and writes nothing.
        cx.update(|cx| take_flip(SIDEBAR, None, false, cx));
        assert!(sidebar(cx));
        // Flipped off from on, it's written; reported again before the page has caught up,
        // as AppKit may, it's left alone.
        cx.update(|cx| take_flip(SIDEBAR, Some(true), false, cx));
        assert!(!sidebar(cx));
        cx.update(|cx| settings::update(cx, |settings| settings.sidebar_open_on_launch = true));
        cx.update(|cx| take_flip(SIDEBAR, Some(false), false, cx));
        assert!(sidebar(cx));
    }
}

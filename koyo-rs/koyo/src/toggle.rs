//! A switch `AppKit` draws: an `NSSwitch` hosted in the window, so every switch, a dialog's or a
//! setting's (`settings_view::native_switch`), is macOS's knob, with its motion and the system's
//! accent. Off macOS, or in a window that can't host a native view (a test's), it's gpui-kit's
//! switch instead.

use std::rc::Rc;

use gpui_kit::component::Disableable as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::{App, Context, IntoElement, RenderOnce, Window};

use crate::theme;

/// What a flip of a [`Toggle`] does, told what the switch is now.
pub type OnFlip = Rc<dyn Fn(bool, &mut App)>;

/// A switch showing `on`, `on_flip` hearing each flip; greyed out, and deaf, while it's not
/// `enabled`.
#[derive(IntoElement, Clone)]
pub struct Toggle {
    id: &'static str,
    on: bool,
    enabled: bool,
    on_flip: OnFlip,
}

impl Toggle {
    pub fn new(id: &'static str, on: bool, on_flip: impl Fn(bool, &mut App) + 'static) -> Self {
        Toggle {
            id,
            on,
            enabled: true,
            on_flip: Rc::new(on_flip),
        }
    }

    /// A switch for a field of `cx`'s view: each flip has `set` write it, and the view redraws.
    pub fn for_view<V: 'static>(
        id: &'static str,
        on: bool,
        cx: &Context<V>,
        set: impl Fn(&mut V, bool) + 'static,
    ) -> Self {
        let view = cx.weak_entity();
        Toggle::new(id, on, move |on, cx| {
            (view.update(cx, |view, cx| {
                set(view, on);
                cx.notify();
            }))
            .ok();
        })
    }

    /// Greys it out while something it would change is under way.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        #[cfg(target_os = "macos")]
        if let Some(switch) = appkit::switch(self.clone(), window, cx) {
            return switch;
        }
        let _ = window;
        let on_flip = self.on_flip;
        Switch::new(self.id)
            .checked(self.on)
            .color(theme::palette(cx).accent)
            .disabled(!self.enabled)
            .on_click(move |on, _, cx| on_flip(*on, cx))
            .into_any_element()
    }
}

#[cfg(target_os = "macos")]
mod appkit {
    //! The `AppKit` half: an `NSSwitch`, and what a flip of it tells.

    use gpui_kit::{
        AnyElement, App, IntoElement as _, ParentElement as _, Styled as _, Window, div,
    };
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::NSSwitch;

    use super::{OnFlip, Toggle};
    use crate::native::{self, Control, switch as native_switch};

    /// The switch `AppKit` draws for `toggle`; none where the window can't host a native view.
    pub fn switch(toggle: Toggle, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let control = Flips(toggle.on_flip);
        let shown = (toggle.on, toggle.enabled);
        let (size, track) = native::host(toggle.id, control, shown, window, cx)?;
        Some(div().w(size.width).child(track).into_any_element())
    }

    /// The switch's control, which tells its flips.
    struct Flips(OnFlip);

    impl Control for Flips {
        type View = NSSwitch;
        /// Whether it's on, and whether it takes a click.
        type Shown = (bool, bool);

        fn make(&self, main: MainThreadMarker) -> Retained<NSSwitch> {
            NSSwitch::new(main)
        }

        fn show(switch: &NSSwitch, (on, enabled): (bool, bool)) {
            native_switch::show(switch, on);
            switch.setEnabled(enabled);
        }

        fn acted(&self, switch: &NSSwitch, shown: Option<(bool, bool)>, cx: &mut App) {
            let on = native_switch::is_on(switch);
            if flipped(shown.map(|(shown, _)| shown), on) {
                (self.0)(on, cx);
            }
        }
    }

    /// Whether `AppKit`'s switch, now `on`, was flipped: not if it was last `shown` so already,
    /// or isn't shown yet.
    fn flipped(shown: Option<bool>, on: bool) -> bool {
        shown.is_some_and(|shown| shown != on)
    }

    #[cfg(test)]
    mod tests {
        use super::flipped;

        #[test]
        fn a_flip_is_told_once_and_only_once_shown() {
            // Not shown yet, a flip is a stray.
            assert!(!flipped(None, false));
            // Flipped off from on, it's told; reported again before the view has caught up, as
            // AppKit may, it's left alone.
            assert!(flipped(Some(true), false));
            assert!(!flipped(Some(false), false));
        }
    }
}

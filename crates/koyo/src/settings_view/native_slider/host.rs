//! The native slider on the page: its track, which `AppKit` draws, over the words for its
//! ends, and what a move of its thumb writes.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, IntoElement as _, ParentElement as _, Styled as _, Window, div, px,
};

use crate::native;
use crate::settings_view::ticked_slider::{Stops, TickedSlider, WIDTH, ends_row};
use crate::theme;

/// What the native slider shows: the stop its thumb is at, and whether it can be moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub at: usize,
    pub enabled: bool,
}

/// `slider` with the track `AppKit` draws, over the words for its ends; none where the window
/// can't host a native view, which falls back to the GPUI slider.
pub fn slider(slider: &TickedSlider, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let shown = Shown {
        at: slider.stops.at(cx),
        enabled: !slider.disabled,
    };
    let (_, track) = native::host(slider.id, slider.stops, shown, window, cx)?;
    let ends = ends_row(slider.ends, theme::palette(cx));
    let framed = div()
        .w(px(WIDTH))
        .flex()
        .flex_col()
        .child(track)
        .child(ends.when(slider.disabled, |ends| ends.opacity(0.45)));
    Some(framed.into_any_element())
}

/// Writes the stop at `index`, `AppKit` having moved the thumb there, unless the slider as it
/// was last `shown` can't be moved: a move sent before it heard, arriving after.
pub fn take_move(stops: Stops, shown: Option<Shown>, index: usize, cx: &mut App) {
    if shown.is_some_and(|shown| shown.enabled) {
        stops.pick(index, cx);
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;
    use crate::key_repeat::KeyRepeat;
    use crate::settings;
    use crate::settings_view::general::SPEEDS;

    const SPEED: Stops = Stops {
        values: &SPEEDS,
        get: |settings| settings.key_repeat.per_second,
        set: |settings, speed| settings.key_repeat.per_second = speed,
    };

    fn set_by_hand(cx: &mut TestAppContext) {
        cx.update(|cx| {
            settings::update(cx, |settings| {
                settings.key_repeat = KeyRepeat {
                    per_second: 100.,
                    ..KeyRepeat::default()
                };
            });
        });
    }

    fn speed(cx: &TestAppContext) -> f32 {
        cx.read(|cx| settings::get(cx).key_repeat.per_second)
    }

    #[gpui_kit::test]
    #[expect(
        clippy::float_cmp,
        reason = "a stop is written as it's listed, and a value by hand left as it was"
    )]
    fn moving_the_thumb_writes_the_stop_unless_the_slider_is_disabled(cx: &mut TestAppContext) {
        cx.update(crate::init);
        set_by_hand(cx);
        let shown = Some(Shown {
            at: 5,
            enabled: true,
        });
        cx.update(|cx| take_move(SPEED, shown, 7, cx));
        assert_eq!(speed(cx), 150.);
        // Let go where it was, the value set by hand stays.
        set_by_hand(cx);
        cx.update(|cx| take_move(SPEED, shown, 5, cx));
        assert_eq!(speed(cx), 100.);
        // Disabled, or not shown yet, a move writes nothing.
        let disabled = Some(Shown {
            at: 5,
            enabled: false,
        });
        cx.update(|cx| take_move(SPEED, disabled, 0, cx));
        cx.update(|cx| take_move(SPEED, None, 0, cx));
        assert_eq!(speed(cx), 100.);
    }
}

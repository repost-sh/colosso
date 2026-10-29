//! What keeps a native slider for as long as its page shows it: the `AppKit` slider, the moves
//! it reports, and where on the page it goes.

use std::sync::Once;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, HitboxBehavior, IntoElement as _, ParentElement as _,
    Styled as _, Task, Window, div, px,
};

use super::appkit::Native;
use crate::settings_view::ticked_slider::{Stops, TickedSlider, WIDTH, ends_row};
use crate::theme;

/// What the native slider shows: the stop its thumb is at, and whether it can be moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub at: usize,
    pub enabled: bool,
}

/// The slider's `AppKit` half and what it was last told to show. Dropped with the page, which
/// takes the slider out of the window.
pub struct Host {
    stops: Stops,
    /// What `AppKit` was last told, so it's told only what changes.
    shown: Option<Shown>,
    native: Option<Native>,
    _moves: Task<()>,
}

/// `slider` with the track `AppKit` draws, over the words for its ends; none where the window
/// can't host a native view, which falls back to the GPUI slider.
pub fn slider(slider: &TickedSlider, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let stops = slider.stops;
    let key = ElementId::NamedInteger(slider.id.into(), 0);
    let host = window.use_keyed_state(key, cx, |window, cx| Host::new(stops, window, cx));
    let shown = Shown {
        at: stops.at(cx),
        enabled: !slider.disabled,
    };
    let height = host.update(cx, |host, _| {
        let native = host.native.as_ref()?;
        if host.shown != Some(shown) {
            native.show(shown);
            host.shown = Some(shown);
        }
        Some(native.height())
    })?;
    // The listener hands AppKit the track's bounds as the page prepaints it, clipped as a
    // hitbox there is: to what of a scroll layer shows, not what it paints beyond. One keeps
    // the scrolling page off a scroll layer too, which would move it without prepainting it.
    let track = div()
        .w_full()
        .h(px(height))
        .on_children_prepainted(move |children, window, cx| {
            if let Some(&bounds) = children.first() {
                let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                if let Some(native) = &host.read(cx).native {
                    native.place(bounds, hitbox.content_mask.bounds, window);
                }
            }
        })
        .child(div().size_full());
    let ends = ends_row(slider.ends, theme::palette(cx));
    let framed = div()
        .w(px(WIDTH))
        .flex()
        .flex_col()
        .child(track)
        .child(ends.when(slider.disabled, |ends| ends.opacity(0.45)));
    Some(framed.into_any_element())
}

impl Host {
    fn new(stops: Stops, window: &mut Window, cx: &mut Context<Host>) -> Host {
        let (tell, mut told) = mpsc::unbounded();
        let native = match Native::new(window, stops.values.len(), tell) {
            Ok(native) => Some(native),
            Err(why) => {
                static SAID: Once = Once::new();
                SAID.call_once(|| log::warn!("the GPUI slider stands in for AppKit's: {why}"));
                None
            }
        };
        // AppKit sends each move as it happens, the thumb still held; they're taken up in the
        // order sent, all that are waiting at once, before the page is drawn again.
        let moves = cx.spawn(async move |host, cx| {
            while let Some(index) = told.next().await {
                let taken = host.update(cx, |host, cx| take_move(host.stops, host.shown, index, cx));
                if taken.is_err() {
                    break;
                }
            }
        });
        cx.on_release(|host: &mut Host, cx| {
            if let Some(native) = host.native.take() {
                native.remove(cx);
            }
        })
        .detach();
        Host {
            stops,
            shown: None,
            native,
            _moves: moves,
        }
    }
}

/// Writes the stop at `index`, `AppKit` having moved the thumb there, unless the slider as it
/// was last `shown` can't be moved: a move sent before it heard, arriving after.
fn take_move(stops: Stops, shown: Option<Shown>, index: usize, cx: &mut App) {
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

//! A slider over a few stops, as macOS's System Settings draws them: a dot under the track at
//! each stop, the thumb snapping from one to the next, and a word under either end for what
//! that end means. It shows no numbers: the stops are spaced to feel evenly apart, and the
//! words say which way is which.

use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window,
    div, px, relative,
};

use crate::settings::{self, Settings};
use crate::theme;

/// How wide the track is.
const WIDTH: f32 = 240.;
/// How big each stop's dot is.
const TICK: f32 = 3.;

/// A setting's value, picked from `stops` along a track. The settings keep the value itself; a
/// value set by hand between two stops is shown at the nearer one, and stays as it was until
/// the thumb is moved.
#[derive(IntoElement)]
pub struct TickedSlider {
    id: &'static str,
    stops: &'static [f32],
    get: fn(&Settings) -> f32,
    set: fn(&mut Settings, f32),
    ends: (&'static str, &'static str),
    disabled: bool,
}

impl TickedSlider {
    /// The slider `id` names, over `stops` from the left, showing what `get` reads from the
    /// settings and writing a stop picked with `set`.
    pub fn new(
        id: &'static str,
        stops: &'static [f32],
        get: fn(&Settings) -> f32,
        set: fn(&mut Settings, f32),
    ) -> Self {
        TickedSlider {
            id,
            stops,
            get,
            set,
            ends: ("", ""),
            disabled: false,
        }
    }

    /// What the words under the left and the right ends say.
    pub fn ends(mut self, left: &'static str, right: &'static str) -> Self {
        self.ends = (left, right);
        self
    }

    /// Dimmed, and deaf to the pointer.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// The stop nearest `value`, by ratio rather than by difference, as the stops are spaced.
pub fn nearest(stops: &[f32], value: f32) -> usize {
    let off = |stop: f32| {
        (value.max(f32::EPSILON) / stop.max(f32::EPSILON))
            .ln()
            .abs()
    };
    (0..stops.len())
        .min_by(|&a, &b| off(stops[a]).total_cmp(&off(stops[b])))
        .unwrap_or(0)
}

impl RenderOnce for TickedSlider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        let TickedSlider {
            id,
            stops,
            get,
            set,
            ends: (left, right),
            disabled,
        } = self;
        let last = stops.len().saturating_sub(1);
        let state = window.use_keyed_state(id, cx, |window, cx| {
            // The track counts stops; the settings, what they stand for.
            cx.subscribe_self(move |_, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event
                    && let Some(&stop) = stops.get(value.end().round() as usize)
                {
                    settings::update(cx, |settings| set(settings, stop));
                }
            })
            .detach();
            // Set elsewhere, by hand in the file say, the thumb goes to it.
            settings::observe_in(window, cx, move |state: &mut SliderState, window, cx| {
                let at = nearest(stops, get(settings::get(cx)));
                if state.value().end().round() as usize != at {
                    state.set_value(at as f32, window, cx);
                }
            })
            .detach();
            SliderState::new()
                .min(0.)
                .max(last as f32)
                .step(1.)
                .default_value(nearest(stops, get(settings::get(cx))) as f32)
        });
        // Under the track, where the thumb's middle is at each stop.
        let ticks = (0..stops.len()).map(|index| {
            let along = if last == 0 {
                0.
            } else {
                index as f32 / last as f32
            };
            div()
                .absolute()
                .top_0()
                .left(relative(along))
                .ml(px(-TICK / 2.))
                .size(px(TICK))
                .rounded_full()
                .bg(p.muted)
        });
        div()
            .relative()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .child(
                div()
                    .debug_selector(|| id.to_owned())
                    .child(Slider::new(&state).bg(p.accent)),
            )
            .child(div().relative().h(px(TICK)).children(ticks))
            .child(
                div()
                    .mt(px(3.))
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(p.muted)
                    .child(left)
                    .child(right),
            )
            .when(disabled, |slider| {
                slider
                    .opacity(0.45)
                    .child(div().absolute().size_full().occlude())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_between_stops_is_shown_at_the_nearer_by_ratio() {
        let stops = [30., 60., 120., 240.];
        assert_eq!(nearest(&stops, 120.), 2);
        // 85 is nearer 60 by difference, nearer 120 by ratio.
        assert_eq!(nearest(&stops, 85.), 2);
        assert_eq!(nearest(&stops, 1.), 0);
        assert_eq!(nearest(&stops, 9000.), 3);
        // The longest first, as a delay's are.
        assert_eq!(nearest(&[500., 250., 125.], 300.), 1);
    }
}

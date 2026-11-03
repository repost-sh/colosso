//! A slider over a few stops, as macOS's System Settings draws them: a dot under the track at
//! each stop, the thumb snapping from one to the next, and a word under either end for what
//! that end means. It shows no numbers: the stops are spaced to feel evenly apart, and the
//! words say which way is which.

use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Div, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Styled as _,
    Window, div, px, relative,
};

use crate::settings::{self, Settings};
use crate::theme::{self, Palette};

/// How wide the track is.
pub const WIDTH: f32 = 240.;
/// How big each stop's dot is.
const TICK: f32 = 3.;

/// A setting picked from a few stops: the stops from the left, how the setting is read, and
/// how a stop is written to it.
#[derive(Clone, Copy)]
pub struct Stops {
    pub values: &'static [f32],
    pub get: fn(&Settings) -> f32,
    pub set: fn(&mut Settings, f32),
}

impl Stops {
    /// The stop the setting is shown at.
    pub fn at(&self, cx: &App) -> usize {
        nearest(self.values, (self.get)(settings::get(cx)))
    }

    /// The value to write once the thumb moves to stop `index`, or none when that's the stop
    /// the setting, at `current`, is shown at already: a value set by hand between two stops
    /// isn't rounded to one by a move that leaves the thumb where it was.
    pub fn picked(&self, current: f32, index: usize) -> Option<f32> {
        let stop = *self.values.get(index)?;
        (index != nearest(self.values, current)).then_some(stop)
    }

    /// Writes the stop at `index`, the thumb having moved there.
    pub fn pick(&self, index: usize, cx: &mut App) {
        if let Some(value) = self.picked((self.get)(settings::get(cx)), index) {
            settings::update(cx, |settings| (self.set)(settings, value));
        }
    }
}

/// A setting's value, picked from its stops along a track. The settings keep the value itself;
/// a value set by hand between two stops is shown at the nearer one, and stays as it was until
/// the thumb is moved.
#[derive(IntoElement)]
pub struct TickedSlider {
    pub id: &'static str,
    pub stops: Stops,
    pub ends: (&'static str, &'static str),
    pub disabled: bool,
}

impl TickedSlider {
    /// The slider `id` names, over `values` from the left, showing what `get` reads from the
    /// settings and writing a stop picked with `set`.
    pub fn new(
        id: &'static str,
        values: &'static [f32],
        get: fn(&Settings) -> f32,
        set: fn(&mut Settings, f32),
    ) -> Self {
        TickedSlider {
            id,
            stops: Stops { values, get, set },
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

/// The words under a slider's two ends.
pub fn ends_row((left, right): (&'static str, &'static str), p: Palette) -> Div {
    div()
        .flex()
        .justify_between()
        .text_xs()
        .text_color(p.muted)
        .child(left)
        .child(right)
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
            ends,
            disabled,
        } = self;
        let last = stops.values.len().saturating_sub(1);
        let state = window.use_keyed_state(id, cx, |window, cx| {
            // The track counts stops; the settings, what they stand for.
            cx.subscribe_self(move |_, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    stops.pick(value.end().round() as usize, cx);
                }
            })
            .detach();
            // Set elsewhere, by hand in the file say, the thumb goes to it.
            settings::observe_in(window, cx, move |state: &mut SliderState, window, cx| {
                let at = stops.at(cx);
                if state.value().end().round() as usize != at {
                    state.set_value(at as f32, window, cx);
                }
            })
            .detach();
            SliderState::new()
                .min(0.)
                .max(last as f32)
                .step(1.)
                .default_value(stops.at(cx) as f32)
        });
        // Under the track, where the thumb's middle is at each stop.
        let ticks = (0..stops.values.len()).map(|index| {
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
            .child(ends_row(ends, p).mt(px(3.)))
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
    use crate::settings_view::general::{DELAYS, SPEEDS};

    /// Stops read and written nowhere: what's tested is which stop they pick.
    fn stops(values: &'static [f32]) -> Stops {
        Stops {
            values,
            get: |_| 0.,
            set: |_, _| {},
        }
    }

    #[test]
    fn each_stop_is_picked_by_its_index_from_another() {
        for values in [&SPEEDS[..], &DELAYS[..]] {
            for (index, &stop) in values.iter().enumerate() {
                assert_eq!(nearest(values, stop), index);
                // From the stop before, or from the first to the second.
                let from = values[if index == 0 { 1 } else { index - 1 }];
                assert_eq!(stops(values).picked(from, index), Some(stop));
            }
        }
    }

    #[test]
    fn a_value_set_by_hand_is_kept_until_the_thumb_moves_off_its_stop() {
        let (speeds, delays) = (stops(&SPEEDS), stops(&DELAYS));
        // 100 rows a second lies between 96 and 120, nearer 96: left there, it stays.
        assert_eq!(speeds.picked(100., 5), None);
        assert_eq!(speeds.picked(100., 6), Some(120.));
        // 300 ms, between 320 and 250, is nearer 320; the longest delay is at the left.
        assert_eq!(delays.picked(300., 0), Some(500.));
        // Past the last stop, nothing.
        assert_eq!(speeds.picked(30., 10), None);
    }

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

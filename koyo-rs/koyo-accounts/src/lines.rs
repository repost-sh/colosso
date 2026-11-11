//! Port of claude-swap's `lines.py` (0fe22a8): a switch line per usage window.
//!
//! One threshold against the binding window (the max of the 5h, 7d and per-model figures)
//! can't say what most people want: about 85-90 on the 5-hour window, which recycles in hours,
//! and 97-99 on the weekly ones, which are the budget. So each window gets its own line (the
//! weekly threshold for every window that resets weekly) and the engine judges each against
//! its own.
//!
//! Three quantities per account, in the engine's words:
//!
//! - raw headroom (`oauth::account_headroom`, `100 - max pct`): can the account serve at all.
//!   Untouched by lines.
//! - [`line_headroom`] per window: policy distance on the 5h scale. A window at its line scores
//!   `100 - threshold` whatever its line, an exhausted one 0, linear on each side (three
//!   anchors: 0% → 100, line → 100 - threshold, 100% → 0). With equal lines it is `100 - pct`
//!   bit for bit, so every margin the engine measures in points keeps its meaning.
//! - the binding window ([`binding_window`]): the least line headroom, over-line windows first.
//!
//! And one margin, [`line_room`]: the points left before the nearest line, each window in its
//! own points. Pure functions over `oauth::relevant_windows`' windows: no settings, no I/O, and
//! no restated percentages.

use crate::oauth::{FIVE_HOUR_LABEL, Window};

/// The switch line for one window: `weekly_threshold` for every window that resets weekly (7d
/// and per-model), `threshold` for the 5h window and for all of them while no weekly line is
/// set.
pub fn window_line(label: &str, threshold: f64, weekly_threshold: Option<f64>) -> f64 {
    match weekly_threshold {
        Some(weekly) if label != FIVE_HOUR_LABEL => weekly,
        _ => threshold,
    }
}

/// The policy headroom of one window on the 5h scale (the module says how).
///
/// The arrangement is exact at the anchors the engine compares at: `line - pct` is exactly 0 at
/// the line, so a window at its line scores exactly `100 - threshold` and a candidate exactly
/// the hysteresis better isn't refused over a 1e-14 residue (measured: `(line - pct) *
/// (threshold / line)` left one for 15 of the integer line pairs); `100 - pct` is exactly 0 at
/// 100%. Equal lines short-circuit to `100 - pct`, so the default setup is raw headroom.
#[expect(
    clippy::float_cmp,
    reason = "equal lines are the default setup, compared exactly as cswap does"
)]
pub fn line_headroom(pct: f64, line: f64, threshold: f64) -> f64 {
    if line == threshold {
        return 100. - pct;
    }
    if pct <= line {
        return (100. - threshold) + threshold * (line - pct) / line;
    }
    (100. - threshold) * (100. - pct) / (100. - line)
}

/// The points left before the nearest switch line, each window in its own points: the least
/// `line - pct`, negative once one is over; `None` without a window.
///
/// Not `line_headroom - (100 - threshold)`: that is the room on the 5h scale, `threshold /
/// line` times the real points on any other line, so 10 points under a 98 weekly line (global
/// 90) read 9.2. Nor the binding window's alone: it is chosen on that scale, so the window
/// nearest its line in its own points can be another.
pub fn line_room<'a>(
    windows: impl IntoIterator<Item = &'a Window>,
    threshold: f64,
    weekly_threshold: Option<f64>,
) -> Option<f64> {
    windows
        .into_iter()
        .map(|window| window_line(&window.label, threshold, weekly_threshold) - window.pct)
        .reduce(f64::min)
}

/// The window holding an account back, with the numbers the engine compares: its raw `pct` and
/// its own `line` (so a message quotes the comparison made, "7d 93% < 97%"), `resets_at` and
/// `line_headroom`, the account's policy headroom.
#[derive(Clone, Debug, PartialEq)]
pub struct BindingWindow {
    pub label: String,
    pub pct: f64,
    pub line: f64,
    pub resets_at: Option<String>,
    pub line_headroom: f64,
}

impl BindingWindow {
    /// At or over its own line, compared on the raw pct, never through a rescaled value, so the
    /// verdict can't drift by an ulp at the line.
    pub fn over(&self) -> bool {
        self.pct >= self.line
    }
}

/// The window with the least policy headroom, over-line windows first; ties keep the first in
/// `relevant_windows` order (5h, 7d, then scoped). `None` without a window.
///
/// Over-line first makes `over()` of the result equal "any window at or over its line" by
/// construction, so the trigger, the landing gate and the all-above census can never disagree
/// with a per-window check. `scale` is the threshold the headroom is measured on (`threshold`
/// when `None`): an account on a line of its own passes its line as `threshold` and the global
/// threshold as `scale`, so accounts on different lines compare on one ruler. At a
/// floating-point tie the scale can change the window chosen, so every caller choosing a window
/// for one account passes the same one.
pub fn binding_window<'a>(
    windows: impl IntoIterator<Item = &'a Window>,
    threshold: f64,
    weekly_threshold: Option<f64>,
    scale: Option<f64>,
) -> Option<BindingWindow> {
    let ruler = scale.unwrap_or(threshold);
    let mut best: Option<BindingWindow> = None;
    for window in windows {
        let line = window_line(&window.label, threshold, weekly_threshold);
        let candidate = BindingWindow {
            label: window.label.clone(),
            pct: window.pct,
            line,
            resets_at: window.resets_at.clone(),
            line_headroom: line_headroom(window.pct, line, ruler),
        };
        let better = best.as_ref().is_none_or(|best| {
            (!candidate.over(), candidate.line_headroom) < (!best.over(), best.line_headroom)
        });
        if better {
            best = Some(candidate);
        }
    }
    best
}

#[cfg(test)]
mod tests;

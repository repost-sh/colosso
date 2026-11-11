//! Port of claude-swap's `pace.py` (0fe22a8): weekly usage pace (issue #125).
//!
//! A weekly window is ahead of pace when more of it is used than the fraction of the week gone
//! so far: 40% used at the 20%-through mark. Only weekly windows (`seven_day` and each `scoped`
//! one): the 5h window resets too fast for pace to mean anything, and the store's poll floor of
//! a few minutes is a large part of five hours but nothing against a week.
//!
//! [`compute_pace`] is pure: it reads an already-fetched window and the reading's `fetched_at`,
//! fetches nothing, and has no say in the poll cadence. Elapsed time is measured to
//! `fetched_at`, not to now, so a reading served stale is judged at the time it was taken.
//!
//! Left out: `compute_pace`'s keyword arguments, which nothing passes (its rules are the
//! constants here), and `projected_exhaustion_ts` and `will_last_to_reset`, which cswap keeps
//! for scripts only: a constant rate has wide error bars against bursty use and would look
//! falsely precise on screen.

use serde_json::Value;

use crate::oauth::iso_timestamp;
use crate::oauth::value::{py_number, str_at};

/// Weekly windows reset every seven days.
pub const WEEKLY_PERIOD_S: f64 = 7. * 86_400.;

/// No marker this long after a weekly reset: elapsed is tiny, the expected share near zero, and
/// almost any use reads as far ahead, a false alarm.
pub const SUPPRESS_AFTER_RESET_S: f64 = 24. * 3600.;

/// The percentage points over the expected share before a marker shows: below it, "ahead" is
/// ordinary variance. A flat gap also means no marker in the week's last day or so, when the
/// percentage tells the story and a maxed window is the switcher's job.
pub const AHEAD_THRESHOLD_PCT: f64 = 15.;

/// One weekly window's pace when its reading was taken.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaceResult {
    /// The share of the week's budget on-schedule use would be at.
    pub expected_pct: f64,
    pub actual_pct: f64,
    /// The time since this window's cycle started.
    pub elapsed_s: f64,
    /// The window's whole cycle.
    pub period_s: f64,
    /// `actual - expected >= AHEAD_THRESHOLD_PCT`.
    pub ahead: bool,
}

/// The pace of one weekly window (`{"pct", "resets_at"}` as stored), or `None` when it can't
/// be computed or wouldn't mean anything: no window, no usable `pct` or `resets_at`, or inside
/// the suppression after a reset.
///
/// `resets_at` is the next reset, the only stamp the API gives; the cycle's start is found by
/// rolling it by whole periods until it lands at or before `fetched_at`, however many cycles
/// stale it is.
pub fn compute_pace(window: Option<&Value>, fetched_at: Option<f64>) -> Option<PaceResult> {
    let window = window?.as_object()?;
    let fetched_at = fetched_at?;
    let pct = window.get("pct").and_then(py_number)?;
    let next_reset = str_at(window, "resets_at").and_then(iso_timestamp)?;
    // (next_reset - fetched_at) mod period: the time left to the next reset, folded into
    // [0, period); the period less that is the time since the cycle started.
    let remaining = python_mod(next_reset - fetched_at, WEEKLY_PERIOD_S);
    let elapsed = if remaining == 0. {
        0.
    } else {
        WEEKLY_PERIOD_S - remaining
    };
    if elapsed < SUPPRESS_AFTER_RESET_S {
        return None;
    }
    let expected_pct = (elapsed / WEEKLY_PERIOD_S * 100.).min(100.);
    Some(PaceResult {
        expected_pct,
        actual_pct: pct,
        elapsed_s: elapsed,
        period_s: WEEKLY_PERIOD_S,
        ahead: pct - expected_pct >= AHEAD_THRESHOLD_PCT,
    })
}

/// Python's float `%`: the result takes the divisor's sign.
fn python_mod(left: f64, right: f64) -> f64 {
    let rem = left % right;
    if rem != 0. && (rem < 0.) != (right < 0.) {
        rem + right
    } else if rem == 0. {
        0f64.copysign(right)
    } else {
        rem
    }
}

#[cfg(test)]
mod tests;

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

use serde_json::Value;

use crate::oauth::{iso_timestamp, value::py_number};

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

/// cswap's keyword arguments to `compute_pace`, at their defaults by [`Default`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaceRules {
    pub period_s: f64,
    pub suppress_after_reset_s: f64,
    pub ahead_threshold_pct: f64,
}

impl Default for PaceRules {
    fn default() -> Self {
        PaceRules {
            period_s: WEEKLY_PERIOD_S,
            suppress_after_reset_s: SUPPRESS_AFTER_RESET_S,
            ahead_threshold_pct: AHEAD_THRESHOLD_PCT,
        }
    }
}

/// [`compute_pace_with`] at the default rules.
pub fn compute_pace(window: Option<&Value>, fetched_at: Option<f64>) -> Option<PaceResult> {
    compute_pace_with(window, fetched_at, &PaceRules::default())
}

/// The pace of one weekly window (`{"pct", "resets_at"}` as stored), or `None` when it can't
/// be computed or wouldn't mean anything: no window, no usable `pct` or `resets_at`, or inside
/// the suppression after a reset.
///
/// `resets_at` is the next reset, the only stamp the API gives; the cycle's start is found by
/// rolling it by whole periods until it lands at or before `fetched_at`, however many cycles
/// stale it is.
pub fn compute_pace_with(
    window: Option<&Value>,
    fetched_at: Option<f64>,
    rules: &PaceRules,
) -> Option<PaceResult> {
    let window = window?.as_object()?;
    let fetched_at = fetched_at?;
    let pct = window.get("pct").and_then(py_number)?;
    let next_reset = window
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(iso_timestamp)?;
    // (next_reset - fetched_at) mod period: the time left to the next reset, folded into
    // [0, period); the period less that is the time since the cycle started.
    let remaining = python_mod(next_reset - fetched_at, rules.period_s);
    let elapsed = if remaining == 0. {
        0.
    } else {
        rules.period_s - remaining
    };
    if elapsed < rules.suppress_after_reset_s {
        return None;
    }
    let expected_pct = (elapsed / rules.period_s * 100.).min(100.);
    Some(PaceResult {
        expected_pct,
        actual_pct: pct,
        elapsed_s: elapsed,
        period_s: rules.period_s,
        ahead: pct - expected_pct >= rules.ahead_threshold_pct,
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

/// When usage would hit 100% at the current rate, as an epoch: for scripts only (a constant
/// rate has wide error bars against bursty use and would look falsely precise on screen).
/// `None` with no measurable rate.
pub fn projected_exhaustion_ts(pace: &PaceResult, fetched_at: f64) -> Option<f64> {
    if pace.elapsed_s <= 0. || pace.actual_pct <= 0. {
        return None;
    }
    let rate = pace.actual_pct / pace.elapsed_s;
    if rate <= 0. {
        return None;
    }
    let remaining = 100. - pace.actual_pct;
    if remaining <= 0. {
        return Some(fetched_at);
    }
    Some(fetched_at + remaining / rate)
}

/// Whether, at the current rate, usage stays under 100% through the reset: for scripts only,
/// as [`projected_exhaustion_ts`]. At a constant rate the projected total is `100 × actual /
/// expected`, so this is false exactly when the window is over expected at all: the marker's
/// signal without its threshold.
pub fn will_last_to_reset(pace: &PaceResult) -> Option<bool> {
    if pace.actual_pct <= 0. {
        return Some(true);
    }
    if pace.elapsed_s <= 0. {
        return None;
    }
    let rate = pace.actual_pct / pace.elapsed_s;
    if rate <= 0. {
        return None;
    }
    let projected = pace.actual_pct + rate * (pace.period_s - pace.elapsed_s);
    Some(projected <= 100.)
}

#[cfg(test)]
mod tests;

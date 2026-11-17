//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): which candidates may take over
//! and in what order (`_rank_candidates` for weekly-first), with the census of who is over the
//! line (`_every_candidate_above_threshold`, `_every_account_above_threshold`) and the anti-flap
//! margin when everyone is (`_all_above_margin_met`). Pure: no emits, no state writes.
//!
//! weekly-first is `best` with consume-first's ranking for a proactive move: the same departure
//! line, landing gate, hysteresis and escapes as `best`. cswap's consume-first branches (its
//! below-threshold trigger, its room bar on escapes) belong to that strategy and are left out.

use std::cmp::Ordering;

use crate::lines::{BindingWindow, line_room};

use super::events::Trigger;
use super::settings::AutoSwitchSettings;
use super::windows::{Readings, seven_day_reset_ts};
use super::{HORIZON_HEADROOM_RATIO, SPENT_HEADROOM_PCT};

/// Whether a candidate beats the active by enough to move, every account over the line.
///
/// The margin is [`HORIZON_HEADROOM_RATIO`] times the active's headroom, capped at
/// [`SPENT_HEADROOM_PCT`] points. Either alone fails a measured case: with no margin an active on
/// 5 points and a peer on 6 moved four times in 35 minutes; with the ratio alone an active on 10
/// held at 90% while an 81% peer sat idle. Both sides are raw headroom (`100 - max pct`). The
/// no-return release reuses this margin, so a recovered account that clears it is never barred.
pub(crate) fn all_above_margin_met(candidate_headroom: f64, active_headroom: f64) -> bool {
    let required =
        (active_headroom * HORIZON_HEADROOM_RATIO).min(active_headroom + SPENT_HEADROOM_PCT);
    candidate_headroom >= required
}

/// Whether every measured candidate is at or over the line. An unmeasured one (no readable
/// window) doesn't block the verdict, as long as one was measured: it can't be chosen.
fn every_candidate_above_threshold(candidates: &[String], readings: &Readings<'_>) -> bool {
    let mut measured = candidates.iter().filter_map(|num| {
        readings
            .get(num)
            .and_then(|reading| reading.binding.as_ref())
            .map(BindingWindow::over)
    });
    let Some(first) = measured.next() else {
        return false;
    };
    first && measured.all(|over| over)
}

/// Whether the active account and every measured candidate are at or over the line: the state
/// where "land somewhere healthy" has no answer. Needs the active's verdict known: without it the
/// engine doesn't know it's in this state, and guessing would relax the landing rule on an
/// ordinary tick.
fn every_account_above_threshold(
    candidates: &[String],
    readings: &Readings<'_>,
    active_over: Option<bool>,
) -> bool {
    active_over == Some(true) && every_candidate_above_threshold(candidates, readings)
}

/// One ranking's inputs: the tick's settings, the trigger, the candidates in sequence order,
/// one snapshot's readings, the active account and its headroom, and the instant decided at.
pub(crate) struct RankInput<'a> {
    pub settings: &'a AutoSwitchSettings,
    pub trigger: Trigger,
    pub candidates: &'a [String],
    pub readings: &'a Readings<'a>,
    pub current: &'a str,
    pub active_headroom: Option<f64>,
    pub now: f64,
}

impl RankInput<'_> {
    pub fn headroom_of(&self, num: &str) -> Option<f64> {
        self.readings.get(num).and_then(|reading| reading.headroom)
    }
}

/// A sort key: Python's tuple compared element by element (a bool as 0 or 1). One ranking's
/// keys all have one shape, the shorter ones padded with a trailing 0.
type Key = [f64; 3];

fn compare(left: &Key, right: &Key) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(a, b)| a.partial_cmp(b).unwrap_or(Ordering::Equal))
        .find(|order| order.is_ne())
        .unwrap_or(Ordering::Equal)
}

fn flag(value: bool) -> f64 {
    f64::from(u8::from(value))
}

/// Filters and ranks the candidates for this trigger: `(ordered, any_known)`, `any_known` when
/// any candidate's usage is readable. `no_return` is the account the bar refuses
/// (`release.rs`).
pub(crate) fn rank_candidates(
    input: &RankInput<'_>,
    no_return: Option<&str>,
) -> (Vec<String>, bool) {
    let settings = input.settings;
    let threshold = settings.threshold;
    let escape = matches!(input.trigger, Trigger::AtLimit | Trigger::Failover);
    let proactive = input.trigger == Trigger::Proactive;
    let active_binding = input
        .readings
        .get(input.current)
        .and_then(|reading| reading.binding.as_ref());
    let active_lh = active_binding
        .map(|binding| binding.line_headroom)
        .or(input.active_headroom);
    let all_above = if escape {
        every_candidate_above_threshold(input.candidates, input.readings)
    } else {
        every_account_above_threshold(
            input.candidates,
            input.readings,
            active_binding.map(BindingWindow::over),
        )
    };

    let mut qualifying: Vec<(Key, String)> = Vec::new();
    let mut any_known = false;
    for num in input.candidates {
        let Some(reading) = input.readings.get(num) else {
            continue;
        };
        let Some(h) = reading.headroom else {
            continue;
        };
        any_known = true;
        if h <= 0. {
            continue;
        }
        if no_return == Some(num.as_str()) {
            continue;
        }
        let binding = reading.binding.as_ref();
        let lh = binding.map_or(h, |binding| binding.line_headroom);
        let room = line_room(&reading.windows, threshold, None).unwrap_or(threshold - (100. - h));
        let over = binding.map_or((100. - h) >= threshold, BindingWindow::over);
        if proactive {
            if over && !all_above {
                continue;
            }
            if all_above {
                if !all_above_margin_met(h, input.active_headroom.unwrap_or(0.)) {
                    continue;
                }
            } else if let (Some(_), Some(active_lh)) = (input.active_headroom, active_lh)
                && lh - active_lh < settings.hysteresis_pct
            {
                continue;
            }
        }
        let key = if all_above {
            [-h, reading.reset_key(input.now), 0.]
        } else if proactive {
            let tight = room < settings.hysteresis_pct;
            let reset_ts = seven_day_reset_ts(reading.usage(), input.now);
            [flag(tight), reset_ts.unwrap_or(f64::INFINITY), -lh]
        } else {
            [flag(over), -lh, 0.]
        };
        qualifying.push((key, num.clone()));
    }
    qualifying.sort_by(|a, b| compare(&a.0, &b.0));
    (
        qualifying.into_iter().map(|(_, num)| num).collect(),
        any_known,
    )
}

//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): what the engine reads of each
//! account's usage. One [`Reading`] per account per tick holds its decision value (a reading, a
//! sentinel, or unknown), its windows, the binding one and its headroom
//! (`_headroom_by_account`), computed once and handed to the poll, the trigger, the ranking and
//! the no-return bar. Also here: the windows a poll reports (`_window_pcts`), the resets ranking
//! reads (`_seven_day_reset_ts`, `_binding_recovery_ts` as [`Reading::reset_key`]) and when an
//! exhausted fleet comes back (`_earliest_recovery`). Pure: no store, no clock.
//!
//! cswap picks the binding window under a weekly line when one is set, and the highest
//! utilization otherwise; with the one line Koyo has, `lines::binding_window` gives that same
//! window (over the line first, then the least headroom, the first of equals).

use std::collections::BTreeMap;

use serde_json::Value;

use crate::lines::{BindingWindow, binding_window};
use crate::oauth::value::str_at;
use crate::oauth::{Window, headroom_of, relevant_windows};
use crate::poll_policy::{limiting_reset_ts, parse_reset_ts};
use crate::usage_store::{Decision, UsageEntry};

use super::MODELS;

/// One account as a tick reads it, borrowed from the collection pass's entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Reading<'a> {
    /// `UsageEntry::decision_value`: a trusted reading or a sentinel, `None` unknown.
    pub decision: Option<Decision<'a>>,
    /// The windows that gate it (`oauth::relevant_windows`).
    pub windows: Vec<Window>,
    /// The window holding it back under the line, `None` without a window.
    pub binding: Option<BindingWindow>,
    /// Raw headroom, `100 - max pct` (`oauth::account_headroom`), `None` unknown.
    pub headroom: Option<f64>,
}

/// Slot → its reading, one per entry the collector returned.
pub(crate) type Readings<'a> = BTreeMap<String, Reading<'a>>;

impl<'a> Reading<'a> {
    pub fn of(entry: &'a UsageEntry, threshold: f64) -> Reading<'a> {
        let decision = entry.decision_value();
        let windows = relevant_windows(decision.and_then(Decision::usage), MODELS);
        Reading {
            decision,
            binding: binding_window(&windows, threshold, None, None),
            headroom: headroom_of(&windows),
            windows,
        }
    }

    /// The reading, when it is one (cswap's `value if isinstance(value, dict) else None`).
    pub fn usage(&self) -> Option<&'a Value> {
        self.decision
            .and_then(Decision::usage)
            .filter(|usage| usage.is_object())
    }

    pub fn is_sentinel(&self, sentinel: &str) -> bool {
        matches!(self.decision, Some(Decision::Sentinel(value)) if value == sentinel)
    }

    /// Under the line: a landing spot.
    pub fn lands(&self) -> bool {
        self.binding.as_ref().is_some_and(|binding| !binding.over())
    }

    /// When the binding window comes back, as a sort key: `inf` when unknown or past.
    ///
    /// The binding window is the one holding the account back (the same set `headroom`
    /// measures, so ranking and headroom can't disagree about which window matters). Not the
    /// weekly window: with every account in the 90s, which 5-hour window rolls over first
    /// decides, and that is minutes away while the weekly one is days away. The binding window
    /// is picked first and then asked for its reset: filtering on the reset first let a lower
    /// window win whenever the binding one's reset was unknown (measured: 7d 95% with no reset
    /// and 5h 40% resetting in an hour returned "back in an hour"). `inf` sorts an account we
    /// can't schedule around last.
    pub fn reset_key(&self, now: f64) -> f64 {
        self.binding
            .as_ref()
            .and_then(|binding| parse_reset_ts(binding.resets_at.as_deref()))
            .filter(|ts| *ts > now)
            .unwrap_or(f64::INFINITY)
    }

    /// Ordered window label → pct: "5h", "7d", as the decision reads them. No label repeats
    /// (no per-model window is read), so cswap's dict of them is the list.
    pub fn window_pcts(&self) -> Vec<(String, f64)> {
        self.windows
            .iter()
            .map(|window| (window.label.clone(), window.pct))
            .collect()
    }
}

/// Every entry's reading under the line.
pub(crate) fn readings(entries: &BTreeMap<String, UsageEntry>, threshold: f64) -> Readings<'_> {
    entries
        .iter()
        .map(|(num, entry)| (num.clone(), Reading::of(entry, threshold)))
        .collect()
}

/// The epoch of an account's 7-day window reset, `None` when unknown or already past.
///
/// weekly-first ranks a proactive move by it: the weekly window is the perishable quota (the
/// 5-hour one recycles too fast to plan around). A stale snapshot can carry a `resets_at` that
/// has since elapsed; read as a real instant it would sort the just-rolled-over account (the
/// least perishable quota of all) as soonest, so past is unknown. Plain `ts <= now`:
/// `RESET_SLACK_S` is poll-scheduling lag, not ranking input.
pub(crate) fn seven_day_reset_ts(usage: Option<&Value>, now: f64) -> Option<f64> {
    let window = usage?.get("seven_day")?.as_object()?;
    parse_reset_ts(str_at(window, "resets_at")).filter(|ts| *ts > now)
}

/// The earliest moment any account is usable again, `None` when that can't be proven.
///
/// Per account it's the latest reset among its windows at 100% or more (an account blocked on
/// both windows isn't usable when the 5h rolls over), then the least across accounts, the active
/// one included (its recovery also ends the blocked state). A blocked account whose exhausted
/// windows carry no reset could recover at any moment, so the answer is unprovable: `None`, and
/// the bounded blocked cadence re-checks rather than sleeping toward another account's later
/// reset.
pub(crate) fn earliest_recovery(readings: &Readings<'_>, now: f64) -> Option<f64> {
    let mut earliest: Option<f64> = None;
    for reading in readings.values() {
        let Some(usage) = reading.usage() else {
            continue;
        };
        if !reading.windows.iter().any(|window| window.pct >= 100.) {
            continue;
        }
        let usable_at = limiting_reset_ts(Some(usage), MODELS).filter(|at| *at > now)?;
        if earliest.is_none_or(|kept| usable_at < kept) {
            earliest = Some(usable_at);
        }
    }
    earliest
}

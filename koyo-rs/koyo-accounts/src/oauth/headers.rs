//! Part of the port of claude-swap's `oauth.py` (0fe22a8): the `anthropic-ratelimit-unified-*`
//! windows of a response made a usage reading, `parse_unified_ratelimit_headers` with
//! `_unified_window` and `_epoch_header_iso`, for Koyo's own use of them: a chat reply's
//! `rate_limit_event` (`docs/claude-accounts.md` 4), which carries the windows Claude Code read
//! from those headers as [`LiveLimits`].
//!
//! The windows are five hours, seven days, and the per-model weekly bucket `7d_oi` (Claude
//! Code's `sevenDayOverageIncluded`). The reading is built as cswap builds one: the
//! utilization a 0–1 fraction made a percentage rounded to two places, the reset epoch seconds
//! made ISO 8601 (Python's `isoformat()`), the `7d_oi` bucket a `weekly_scoped` limit under
//! cswap's name for it, and the whole normalised by [`super::build_usage_result`]. A five-hour
//! or seven-day window whose utilization is unusable (not finite, negative) fails the reading,
//! as cswap's; an unusable `7d_oi` bucket is left out; an unusable reset drops only
//! `resets_at`.
//!
//! Left out: reading the headers themselves (Claude Code parses them; Koyo gets the windows),
//! so `_unified_window_present` too, and the `rejected` status entry: the agent host's event
//! carries the reply's status, but [`LiveLimits`] takes only the windows. The header probe that
//! sends a request to read them is left out with it (`oauth/mod.rs`).

use serde_json::{Map, Value, json};

use super::reset::utc_at;

/// cswap's `_UNIFIED_SCOPED_WINDOWS`: the model name the usage API's `weekly_scoped` limits
/// give the `7d_oi` bucket, so a configured model matches either source alike.
pub const SEVEN_DAY_OI_MODEL: &str = "Fable";

/// One window of a response's rate limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveWindow {
    /// The fraction used, 0–1 (past 1 when usage ran past the cap).
    pub utilization_fraction: f64,
    /// When the window resets, epoch seconds, when the response said.
    pub resets_at: Option<f64>,
}

/// What one chat reply said of the account's limits.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveLimits {
    pub five_hour: Option<LiveWindow>,
    pub seven_day: Option<LiveWindow>,
    /// The per-model weekly bucket `7d_oi` ([`SEVEN_DAY_OI_MODEL`]'s).
    pub seven_day_overage_included: Option<LiveWindow>,
}

/// cswap's `_unified_window`: `(percent, resets_at ISO)`, or `None` for an unusable
/// utilization.
fn unified_window(window: LiveWindow) -> Option<(f64, Option<String>)> {
    let fraction = window.utilization_fraction;
    if !fraction.is_finite() || fraction < 0. {
        return None;
    }
    Some((
        round2(fraction * 100.),
        window.resets_at.and_then(epoch_header_iso),
    ))
}

/// Python's `round(x, 2)`: the shortest decimal correctly rounded, ties to even.
fn round2(value: f64) -> f64 {
    format!("{value:.2}").parse().unwrap_or(value)
}

/// cswap's `_epoch_header_iso`: epoch seconds as `datetime.fromtimestamp(ts, utc).isoformat()`,
/// microseconds shown only when there are some; `None` when unusable.
fn epoch_header_iso(seconds: f64) -> Option<String> {
    if !seconds.is_finite() || seconds <= 0. {
        return None;
    }
    let at = utc_at(seconds)?;
    let shape = if at.timestamp_subsec_micros() == 0 {
        "%Y-%m-%dT%H:%M:%S+00:00"
    } else {
        "%Y-%m-%dT%H:%M:%S%.6f+00:00"
    };
    Some(at.format(shape).to_string())
}

impl LiveLimits {
    /// The usage reading these windows make at `now`, as [`super::build_usage_result`]
    /// normalises it; `None` when nothing usable came (no window, or a five-hour or seven-day
    /// one that can't be read).
    pub fn reading(&self, now: f64) -> Option<Map<String, Value>> {
        match super::build_usage_result(&self.raw_usage()?, now) {
            Ok(Some(Value::Object(reading))) => Some(reading),
            _ => None,
        }
    }

    /// The raw usage-API shape cswap's `parse_unified_ratelimit_headers` makes of the headers,
    /// or `None` when a five-hour or seven-day window can't be read (the reading fails).
    fn raw_usage(&self) -> Option<Value> {
        let mut data = Map::new();
        for (key, window) in [("five_hour", self.five_hour), ("seven_day", self.seven_day)] {
            let Some(window) = window else {
                continue;
            };
            let (pct, resets_at) = unified_window(window)?;
            let mut entry = json!({"utilization": pct});
            if let Some(resets_at) = resets_at {
                entry["resets_at"] = Value::from(resets_at);
            }
            data.insert(key.to_owned(), entry);
        }
        if let Some((pct, resets_at)) = self.seven_day_overage_included.and_then(unified_window) {
            let mut limit = json!({
                "kind": "weekly_scoped",
                "scope": {"model": {"display_name": SEVEN_DAY_OI_MODEL}},
                "percent": pct,
            });
            if let Some(resets_at) = resets_at {
                limit["resets_at"] = Value::from(resets_at);
            }
            data.insert("limits".to_owned(), json!([limit]));
        }
        Some(Value::Object(data))
    }
}

//! Port of claude-swap's `poll_policy.py` (0fe22a8): the cadence of `/api/oauth/usage`, every
//! number in one place.
//!
//! The endpoint budgets non-first-party clients: a ~60-minute window of ~28-30 requests per
//! identity × User-Agent class (measured 2026-07-11: a rested identity took 30 requests before
//! the first 429; the 429s after a drain ended exactly when the drain aged 60 minutes; steady
//! 1/180 s polling then ran 96 minutes with none). It is no refilling bucket: capacity returns
//! only as old requests age out of the trailing hour, so a burst saturates the identity for up
//! to an hour and pausing doesn't restore headroom early.
//!
//! The identity depends on the org's 429 regime. Under the fixed-deadline one it is the
//! account (a fresh token was blocked 135 s after issue, 2026-07-28); under the
//! `Retry-After: 0` edge it is the access token (2026-07-29). Plan for the account-scoped case:
//! a re-login can't be relied on to clear a block, and two machines with different tokens for
//! one account may share one budget, which `POST_429_BACKOFF_MULT` exists to converge.
//!
//! The constants lean only on the robust parts: a sustained rate under the cap and an hour-scale
//! recovery. The target is at most ~1 request / 3 minutes on average (20/hour against ~28-30),
//! leaving headroom for manual reads, wake-up catch-up and the bounded urgent mode. In steady
//! state the log shows no 429. A post-burst 429 does not reliably clear at its stated deadline
//! (20 of 35 lapsed blocks re-blocked within 900 s of it), hence the store's `Retry-After` plus
//! `usage_store::RETRY_AFTER_MARGIN_S`.
//!
//! Plans computed here are stored per account (`nextPollAt`/`pollIntervalS`) by whichever
//! collector fetched, so every surface inherits one cadence. Koyo's own cadence for the active
//! account and the open panel (`docs/claude-accounts.md` 4) sits on top of these rules.
//!
//! cswap draws the jitter from `random.random`; here [`PlanInput::rng`] carries the drawn value
//! ([`random_unit`] in production), so tests are as deterministic as cswap's fixture makes them.

use serde_json::Value;

use crate::lines::window_line;
use crate::oauth::{self, iso_timestamp};
use crate::random::random_u64;

/// The freshness floor every collector shares: a reading younger than this is served without a
/// fetch, so one token's sustained rate is at most 1/`SERVE_TTL_S` however many views are open.
pub const SERVE_TTL_S: f64 = 180.;

/// The normal cadence floor: movement halves an interval down to this, never below.
pub const MIN_INTERVAL_S: f64 = 180.;

/// Urgent mode: the active account, with a window that rose this poll to within
/// [`ESCALATION_MARGIN_PCT`] of its own line, burning toward a limit. Bounded by construction:
/// the line is crossed (the engine switches) or the rise stops (back to `MIN_INTERVAL_S`);
/// at worst margin / delta ≈ 15 polls an episode, inside the hourly window.
pub const URGENT_INTERVAL_S: f64 = 60.;

/// Decay ceilings for an account whose usage isn't moving.
pub const ACTIVE_MAX_INTERVAL_S: f64 = 300.;
pub const CANDIDATE_DEFAULT_INTERVAL_S: f64 = 300.;
pub const CANDIDATE_MAX_INTERVAL_S: f64 = 600.;

/// Exhaustion is stable enough to poll slowly, not to stop until a reported reset: quota grants
/// and corrections can free an account early, and its status must not age into unknown while
/// the scheduler waits. Six requests an hour stays under the budget.
pub const EXHAUSTED_INTERVAL_S: f64 = 600.;

/// A binding pct that moved at least this much between polls is being used somewhere: tighten.
pub const MOVEMENT_DELTA_PCT: f64 = 1.;

/// ±fraction on each scheduled interval, so independent processes drift apart.
pub const JITTER_FRAC: f64 = 0.1;

/// After a 429 with `Retry-After: 0` (the saturated edge): probe at most every 5 minutes, so
/// ageing-out (up to ~30/hour) outpaces the probing.
pub const EDGE_BACKOFF_S: f64 = 300.;
/// While a 429 was seen within [`RECENT_429_WINDOW_S`], the cadence is floored here so freed
/// capacity accumulates; the window is the saturation horizon.
pub const POST_429_MIN_INTERVAL_S: f64 = 360.;
pub const RECENT_429_WINDOW_S: f64 = 3600.;

/// AIMD on a contended budget: every machine polling one account shares it, none sees the
/// others, and the endpoint shows no remaining count. While 429s recur each successful poll
/// grows the interval ×1.5 toward a ceiling wider than the candidate one, so machines retreat
/// until their combined rate fits; movement without a recent 429 brings it back down.
pub const POST_429_BACKOFF_MULT: f64 = 1.5;
pub const POST_429_MAX_INTERVAL_S: f64 = 1800.;

/// The engine escalates to a full candidate refresh when an active window is this close to its
/// line; urgent mode keys on the same band.
pub const ESCALATION_MARGIN_PCT: f64 = 15.;

/// Whether a window is within [`ESCALATION_MARGIN_PCT`] of its own line: a switch could be near,
/// so the engine refreshes every candidate, and a window rising into the band puts the active
/// account's poll in urgent mode.
pub fn near_line(window: &oauth::Window, threshold: f64, weekly_threshold: Option<f64>) -> bool {
    window.pct >= window_line(&window.label, threshold, weekly_threshold) - ESCALATION_MARGIN_PCT
}

/// Never schedule a poll later than a known window reset (plus this): the reading is obsolete
/// the moment the window rolls over.
pub const RESET_SLACK_S: f64 = 60.;

/// The utilization of the binding (worst) relevant window.
pub fn binding_pct(usage: Option<&Value>, models: &[&str]) -> Option<f64> {
    oauth::account_headroom(usage, models).map(|headroom| 100. - headroom)
}

/// When the last of the windows at 100% or more resets: the account usable again.
pub fn limiting_reset_ts(usage: Option<&Value>, models: &[&str]) -> Option<f64> {
    oauth::relevant_windows(usage, models)
        .iter()
        .filter(|window| window.pct >= 100.)
        .filter_map(|window| parse_reset_ts(window.resets_at.as_deref()))
        .reduce(f64::max)
}

/// The next relevant-window reset after `now`, whatever its utilization.
pub fn earliest_future_reset_ts(usage: Option<&Value>, now: f64, models: &[&str]) -> Option<f64> {
    oauth::relevant_windows(usage, models)
        .iter()
        .filter_map(|window| parse_reset_ts(window.resets_at.as_deref()))
        .filter(|ts| *ts > now)
        .reduce(f64::min)
}

/// A reset time's epoch, `Z` read as UTC.
pub fn parse_reset_ts(resets_at: Option<&str>) -> Option<f64> {
    let resets_at = resets_at.filter(|text| !text.is_empty())?;
    iso_timestamp(&resets_at.replace('Z', "+00:00"))
}

/// What [`plan_after_fetch`] reads: cswap's keyword arguments.
#[derive(Clone, Copy, Debug)]
pub struct PlanInput<'a> {
    pub prev_interval_s: Option<f64>,
    pub prev_usage: Option<&'a Value>,
    pub new_usage: Option<&'a Value>,
    pub is_active: bool,
    pub threshold: f64,
    pub models: &'a [&'a str],
    pub recent_429: bool,
    pub now: f64,
    pub weekly_threshold: Option<f64>,
    /// cswap's `rng()`: a draw in [0, 1) ([`random_unit`]); 0.5 is no jitter.
    pub rng: f64,
}

/// `(next_poll_at, interval_s)` for an account just fetched successfully.
///
/// Movement (the binding pct changed by [`MOVEMENT_DELTA_PCT`] or more) halves the interval,
/// floored at [`MIN_INTERVAL_S`], or drops it to [`URGENT_INTERVAL_S`] when one of the active
/// account's windows rose that much into the band under its own line. No movement backs off
/// ×1.5 toward the account's ceiling; unknown utilization takes the default. A recent 429
/// floors the cadence at [`POST_429_MIN_INTERVAL_S`] (and suppresses urgency) and grows it
/// toward [`POST_429_MAX_INTERVAL_S`]. The time gets [`JITTER_FRAC`] noise and is never later
/// than the next window reset plus [`RESET_SLACK_S`]; an account at its limit keeps a slow
/// bounded poll rather than sleeping until that reset, so an early grant is seen.
pub fn plan_after_fetch(input: &PlanInput<'_>) -> (f64, f64) {
    let models = input.models;
    let default = if input.is_active {
        MIN_INTERVAL_S
    } else {
        CANDIDATE_DEFAULT_INTERVAL_S
    };
    let ceiling = if input.is_active {
        ACTIVE_MAX_INTERVAL_S
    } else {
        CANDIDATE_MAX_INTERVAL_S
    };
    let base = input
        .prev_interval_s
        .filter(|v| *v != 0.)
        .unwrap_or(default);
    let mut interval = match (
        binding_pct(input.prev_usage, models),
        binding_pct(input.new_usage, models),
    ) {
        (Some(prev), Some(new)) if (new - prev).abs() >= MOVEMENT_DELTA_PCT => {
            MIN_INTERVAL_S.max(base / 2.)
        }
        // Floored, so a sub-floor base (urgent mode's 60 s) snaps back to the normal cadence
        // once movement stops instead of decaying through 90 s and 135 s polls.
        (Some(_), Some(_)) => ceiling.min(MIN_INTERVAL_S.max(base * 1.5)),
        _ => default,
    };
    // Per window, by label, rising only: the window that matters may not be the max (5h
    // climbing under a flat higher 7d), and one that just dropped into the band rolled over.
    let prev_windows = oauth::relevant_windows(input.prev_usage, models);
    let rising_in_band = oauth::relevant_windows(input.new_usage, models)
        .iter()
        .any(|window| {
            prev_windows
                .iter()
                .rfind(|prev| prev.label == window.label)
                .is_some_and(|prev| {
                    window.pct - prev.pct >= MOVEMENT_DELTA_PCT
                        && near_line(window, input.threshold, input.weekly_threshold)
                })
        });
    if input.is_active && rising_in_band && !input.recent_429 {
        interval = URGENT_INTERVAL_S;
    }
    if input.recent_429 {
        // AIMD: grow from the last interval toward the wider 429 ceiling, floored for the
        // first 429.
        let increased = (base * POST_429_BACKOFF_MULT).max(POST_429_MIN_INTERVAL_S);
        interval = POST_429_MAX_INTERVAL_S.min(interval.max(increased));
    }
    let exhausted =
        oauth::account_headroom(input.new_usage, models).is_some_and(|headroom| headroom <= 0.);
    if exhausted {
        // Keep probing: quota can be granted before the advertised time. A wider post-429
        // interval stands.
        interval = interval.max(EXHAUSTED_INTERVAL_S);
    }
    let mut next_poll = input.now + jittered(interval, input.rng);
    let reset = if exhausted {
        limiting_reset_ts(input.new_usage, models).filter(|ts| *ts > input.now)
    } else {
        earliest_future_reset_ts(input.new_usage, input.now, models)
    };
    if let Some(reset) = reset {
        next_poll = next_poll.min(reset + RESET_SLACK_S);
    }
    (next_poll, interval)
}

/// `interval` with ±[`JITTER_FRAC`] noise, from `rng`, a uniform draw in [0, 1).
pub(crate) fn jittered(interval: f64, rng: f64) -> f64 {
    interval * (1. + JITTER_FRAC * (2. * rng - 1.))
}

/// A uniform draw in [0, 1), cswap's `random.random()` for [`PlanInput::rng`].
pub fn random_unit() -> f64 {
    (random_u64() >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests;

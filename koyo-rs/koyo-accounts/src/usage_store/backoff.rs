//! Part of the port of claude-swap's `usage_store.py` (0fe22a8): how long a failed fetch waits
//! (`_failure_backoff_s`), how long stale data stays trusted (`_earliest_reset`,
//! `_rate_limited_trust_ok`), and a refused probe's deadline (`_rejection_backoff_until`,
//! `_rejection_deadline`), with the numbers behind them. cswap's measurements are condensed
//! here; `BACKOFF_CAP_S` and `RETRY_AFTER_MARGIN_ABOVE_S` are the spec's (`mod.rs` says why).

use serde_json::Value;

use crate::oauth;
use crate::oauth::value::{py_number, str_at};
use crate::oauth::{INVALID_GRANT_ERROR, NO_REFRESH_TOKEN_ERROR};
use crate::poll_policy::{EDGE_BACKOFF_S, parse_reset_ts};

/// Deliberate staleness (backoff, the scheduler's cadence) extends decision trust past
/// `STALE_OK_S`, never past this: a forever-failing account must read as unknown at last, so
/// the unknown-path machinery takes over. It overrides even a longer `Retry-After`: trust must
/// never be server-controlled and unbounded.
pub const TRUST_MAX_AGE_S: f64 = 3600.;

/// A usage-endpoint 429 throttles polling, not the account's quota: usage only rises within a
/// window, so the last good reading is a lower bound until that window resets, and is trusted
/// until then (as Claude Code 2.1.208 shows last-known usage when rate-limited). This bounds
/// 429-stale data with no `resets_at`. Other failures use [`TRUST_MAX_AGE_S`]: a timeout is no
/// evidence the reading still holds.
pub const RATE_LIMIT_TRUST_MAX_AGE_S: f64 = 7200.;

/// The failure backoff without a `Retry-After`: 30 s · 2^(n-1), capped at 15 min (the spec,
/// Orca's cadence; cswap caps at 600 s).
pub const BACKOFF_BASE_S: f64 = 30.;
pub const BACKOFF_CAP_S: f64 = 900.;
/// The exponent's clamp: a permanently failing account counts forever, and the curve has
/// saturated long before.
pub const BACKOFF_MAX_SHIFT: i64 = 32;

/// Honouring a 429's `Retry-After` exactly lands the retry on the server's deadline, where it
/// isn't reliably ready: measured over one machine's log (2026-08-03), 20 of 35 block lapses
/// re-blocked within 900 s of their own deadline (+2 s to +887 s), each for a fresh hour; the
/// next was +1004 s. The margin is absolute: `Retry-After` counts down to a fixed deadline, so
/// a machine polling into a block another opened sees only the remainder.
pub const RETRY_AFTER_MARGIN_S: f64 = 900.;
/// The margin applies only to an ask over this: short asks were measured accurate (300 meant a
/// 300 s block). cswap's threshold, which it wrote as `> BACKOFF_CAP_S` while that was 600.
pub const RETRY_AFTER_MARGIN_ABOVE_S: f64 = 600.;
/// Bounds a 429's ask plus the margin, so a pathological header can't park an account for
/// hours: 40 of 41 observed blocks opened at exactly 3600, and 3600 + 900 is this. It sits
/// inside the 429 trust ([`RATE_LIMIT_TRUST_MAX_AGE_S`]).
pub const RETRY_AFTER_FLOOR_CAP_S: f64 = 4500.;

/// A dead refresh-token lineage (`invalid_grant`) never recovers on its own; one server answer
/// is definitive, so one strike quarantines the account until a re-login.
pub const AUTH_DEAD_STRIKES: i64 = 1;

/// Errors that prove the stored credential permanently unusable; only these strike.
pub const PERMANENT_AUTH_ERRORS: [&str; 2] = [INVALID_GRANT_ERROR, NO_REFRESH_TOKEN_ERROR];

/// The soonest relevant window's reset, or `None`: once the soonest rolls over the whole
/// reading is obsolete, so a later window can't rescue it. A window without `resets_at` adds
/// nothing rather than a guess.
pub fn earliest_reset(last_good: Option<&Value>, models: &[&str]) -> Option<f64> {
    oauth::relevant_windows(last_good, models)
        .iter()
        .filter_map(|window| parse_reset_ts(window.resets_at.as_deref()))
        .reduce(f64::min)
}

/// Whether 429-stale `last_good` is still trustworthy for decisions:
/// `now < min(earliest relevant-window reset, fetched_at + RATE_LIMIT_TRUST_MAX_AGE_S)`. The
/// earliest reset, not the latest; the ceiling holds whether or not a reset is known, so a
/// far-future or malformed `resets_at` is bounded too, and missing metadata only tightens it.
pub fn rate_limited_trust_ok(
    last_good: Option<&Value>,
    age_s: Option<f64>,
    now: f64,
    models: &[&str],
) -> bool {
    let Some(age_s) = age_s else {
        return false;
    };
    let ceiling = now + (RATE_LIMIT_TRUST_MAX_AGE_S - age_s);
    let bound = earliest_reset(last_good, models).map_or(ceiling, |soonest| soonest.min(ceiling));
    now < bound
}

/// `rejected.retry_after_s` of a reading, when a positive number.
fn rejection_retry_after(usage: Option<&Value>) -> Option<(&serde_json::Map<String, Value>, f64)> {
    let rejected = usage?.get("rejected")?.as_object()?;
    let retry_after = rejected.get("retry_after_s").and_then(py_number)?;
    (retry_after > 0.).then_some((rejected, retry_after))
}

/// When a reading from a refused probe (`rejected` with a `Retry-After`) allows the next
/// request, or `None`. Stored as `backoffUntil`, which every eligibility check honours; bounded
/// like a 429's ask.
pub fn rejection_backoff_until(usage: Option<&Value>, now: f64) -> Option<f64> {
    let (_, retry_after) = rejection_retry_after(usage)?;
    Some(now + retry_after.min(RETRY_AFTER_FLOOR_CAP_S))
}

/// The absolute end of a refusal's `Retry-After` a reading carries, bounded like
/// [`rejection_backoff_until`], or `None` when unusable or past. For a reading taken on another
/// machine the `Retry-After` was relative to its fetch, so only the absolute
/// `rejected.resets_at` carries over; without a positive `retry_after_s` that stamp is the
/// quota reset, not a deadline, and installs no backoff here either.
pub fn rejection_deadline(usage: Option<&Value>, now: f64) -> Option<f64> {
    let (rejected, _) = rejection_retry_after(usage)?;
    let at = parse_reset_ts(str_at(rejected, "resets_at"))?;
    (at > now).then(|| at.min(now + RETRY_AFTER_FLOOR_CAP_S))
}

/// Seconds to stay in backoff after a failed fetch.
///
/// - No `Retry-After`: the curve, [`BACKOFF_BASE_S`] doubling to [`BACKOFF_CAP_S`].
/// - `Retry-After: 0` on a 429: the saturated-budget edge (the trailing hour's budget frees only
///   as old requests age out), so at least [`EDGE_BACKOFF_S`], capped. On any other status (a
///   Cloudflare 503 "retry now") it is the plain curve: the edge was measured on 429s only.
/// - `Retry-After: N`: the server's ask, honoured whole (an hour-scale block is reported
///   accurately and not re-armed by probing, so re-probing inside it only wastes requests), plus
///   [`RETRY_AFTER_MARGIN_S`] when a 429's ask is over [`RETRY_AFTER_MARGIN_ABOVE_S`]; our own
///   curve may still wait longer. A block whose remainder has fallen under that takes no margin
///   and still retries near its deadline: nothing local tells it from a short burst block.
///
/// Every ask is then capped by the trust ceiling its own arm uses: [`RETRY_AFTER_FLOOR_CAP_S`]
/// for a 429, [`TRUST_MAX_AGE_S`] for any other status (`Retry-After` is read on every status,
/// and a 503 asking a day must not park a row past its own trust). The cap bounds the park, not
/// the time a row is both un-pollable and untrusted: trust runs from the last success, which a
/// chain of failures never moves. Nothing trims a wait to a trust deadline either: measured,
/// that never salvaged trust and only landed retries on the server's deadline.
pub fn failure_backoff_s(
    consecutive_failures: i64,
    retry_after_s: Option<f64>,
    rate_limited: bool,
) -> f64 {
    let shift = (consecutive_failures - 1).clamp(0, BACKOFF_MAX_SHIFT);
    let computed = (BACKOFF_BASE_S * 2f64.powi(shift as i32)).min(BACKOFF_CAP_S);
    let Some(retry_after) = retry_after_s else {
        return computed;
    };
    if retry_after == 0. {
        if !rate_limited {
            return computed;
        }
        return computed.clamp(EDGE_BACKOFF_S, BACKOFF_CAP_S);
    }
    let mut asked = retry_after;
    if retry_after > RETRY_AFTER_MARGIN_ABOVE_S && rate_limited {
        asked = retry_after + RETRY_AFTER_MARGIN_S;
    }
    let park_cap = if rate_limited {
        RETRY_AFTER_FLOOR_CAP_S
    } else {
        TRUST_MAX_AGE_S
    };
    asked.min(park_cap).max(computed)
}

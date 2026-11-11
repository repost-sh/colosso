//! Port of claude-swap's `oauth.py` (0fe22a8): OAuth tokens and the usage API of Claude Code
//! accounts. The token helpers and fingerprints are here; the refresh grant and the profile are
//! in `refresh.rs`, the usage endpoint, its results and its error kinds in `usage.rs`, the
//! windows decisions run on in `windows.rs`, reset strings in `reset.rs`, and the usage
//! sentinels of `json_output.py` in `sentinels.rs`.
//!
//! Requests go through `env.http` (the system `curl`, headers on stdin) with cswap's URLs,
//! headers, client id and timeouts. cswap's `User-Agent` is kept: the token endpoint answers 403
//! without one, and the usage endpoint's measured budget is per identity and User-Agent class
//! (`poll_policy`). Where cswap reads `datetime.now()`, the caller passes `now` (`env.now()`).
//!
//! Left out: the header probe that reads a setup-token's usage from a 1-token model request
//! (`request_usage_via_probe`, `_probe_request`, `_scoped_probe_limits`, `_usage_from_probe`,
//! `parse_unified_ratelimit_headers`, `_unified_window*`, `_epoch_header_iso`,
//! `_header_reading_complete`), API-key paths, and the refresh inside a usage fetch
//! (`refresh_via`, `persist_credentials`, `_persist`, `fetch_usage_for_account`): Koyo never
//! refreshes a token to read usage (`docs/claude-accounts.md` 4), see
//! [`try_fetch_usage_for_account`].

mod refresh;
mod reset;
pub mod sentinels;
mod usage;
pub(crate) mod value;
mod windows;

#[cfg(test)]
mod tests;

use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

pub use refresh::{
    AccountIdentity, DETERMINISTIC_REFRESH_ERRORS, PROFILE_401_WARNING, RefreshOutcome,
    fetch_oauth_profile, parse_token_account, refresh_oauth_credentials,
    try_refresh_oauth_credentials,
};
pub use reset::{
    Iso, ResetError, format_countdown, format_reset, fresh_reset_strings, parse_iso,
    reset_clock_string,
};
pub(crate) use reset::{iso_timestamp, utc_at};
pub use sentinels::*;
pub use usage::{
    FetchError, InvalidUsageResponse, ORG_POLICY_ERROR, RETRY_AFTER_BOUND_S, TOKEN_EXPIRED_ERROR,
    UsageOutcome, build_usage_result, classify_usage_error, fetch_usage, log_usage_failure,
    parse_retry_after, reported_window, request_usage_data, try_fetch_usage_for_account,
    usage_failure_line, window_entry,
};
pub use windows::{FIVE_HOUR_LABEL, REJECTED_LABEL, Window, account_headroom, relevant_windows};

use value::{py_number, truthy_at};

pub const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20";
pub const OAUTH_EXPIRY_BUFFER_MS: f64 = 5. * 60. * 1000.;
pub const OAUTH_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
/// Koyo's own, where cswap sends `claude-swap/1.0`: the token endpoint refuses a request without
/// one (403), and the usage endpoint's budget counts by client, so Koyo doesn't pass for another.
pub const USER_AGENT: &str = "koyo-accounts/1.0";

/// What `claude setup-token` grants: inference, and not the usage endpoint.
pub const SETUP_TOKEN_SCOPES: [&str; 1] = ["user:inference"];

/// How far ahead a stored login's expiry is worth showing. Claude Code's own notice fires in
/// the last three days; a week leaves time to plan several re-logins without nagging for a month.
pub const LOGIN_DUE_WITHIN_S: f64 = 7. * 86_400.;

/// The OAuth access token of a credentials JSON string.
pub fn extract_access_token(credentials: &str) -> Option<String> {
    let data: Value = serde_json::from_str(credentials).ok()?;
    data.get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(str::to_owned)
}

/// The `claudeAiOauth` object of a credentials JSON string.
pub fn extract_oauth_data(credentials: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str(credentials).ok()? {
        Value::Object(mut data) => match data.remove("claudeAiOauth")? {
            Value::Object(oauth) => Some(oauth),
            _ => None,
        },
        _ => None,
    }
}

fn sha256_hex(text: &str) -> String {
    use std::fmt::Write as _;
    Sha256::digest(text.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            write!(hex, "{byte:02x}").ok();
            hex
        })
}

/// A stable identity fingerprint for a stored credential.
///
/// The refresh token's hash when there is one: it survives access-token rotation, so two
/// generations of one OAuth lineage compare equal. Otherwise the whole content's (API keys and
/// setup tokens never rotate, so content is lineage). `None` only for empty input: a caller
/// asking "did the credential change?" must never get `None` for real bytes.
pub fn credential_fingerprint(credentials: &str) -> Option<String> {
    if credentials.is_empty() {
        return None;
    }
    let oauth = extract_oauth_data(credentials).filter(|data| !data.is_empty());
    match oauth
        .as_ref()
        .and_then(|data| value::str_at(data, "refreshToken"))
    {
        Some(token) if !token.is_empty() => Some(format!("sha256:{}", sha256_hex(token))),
        _ => Some(format!("sha256-full:{}", sha256_hex(credentials))),
    }
}

/// The access token's hash alone: the part that rotates within a lineage, so a refused token
/// and its replacement compare unequal.
pub fn access_token_fingerprint(credentials: &str) -> Option<String> {
    let oauth = extract_oauth_data(credentials)?;
    let token = value::str_at(&oauth, "accessToken").filter(|token| !token.is_empty())?;
    Some(format!("sha256-at:{}", sha256_hex(token)))
}

/// The raw `refreshTokenExpiresAt` (epoch milliseconds), or `None` when absent or unusable:
/// the one reader of the field behind [`login_expires_at_iso`] and [`login_expires_at_epoch`].
fn refresh_token_expires_at_ms(credentials: &str) -> Option<f64> {
    let oauth = extract_oauth_data(credentials)?;
    let at = value::number(oauth.get("refreshTokenExpiresAt")?)?;
    (at.is_finite() && at > 0.).then_some(at)
}

/// When the stored login itself lapses, as ISO-8601 UTC, or `None`.
///
/// Claude Code stores `refreshTokenExpiresAt` (epoch milliseconds) beside the access token's
/// `expiresAt`. The access token is renewed from the refresh token on its own; the refresh
/// token only by a fresh `/login`, after which nothing but logging in again helps, so this is
/// the date worth showing before it comes. Logins older than the field carry nothing: unknown,
/// never "now".
pub fn login_expires_at_iso(credentials: &str) -> Option<String> {
    let at = utc_at(refresh_token_expires_at_ms(credentials)? / 1000.)?;
    Some(at.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// [`login_expires_at_iso`] in epoch seconds.
pub fn login_expires_at_epoch(credentials: &str) -> Option<f64> {
    refresh_token_expires_at_ms(credentials).map(|at| at / 1000.)
}

/// Seconds until a stored login must be redone, when that is worth saying: `None` when the
/// stamp is unknown, further out than [`LOGIN_DUE_WITHIN_S`], or the slot is quarantined (its
/// sentinel already reads "re-login needed"); else the seconds left, `0` once past.
pub fn login_time_left(expires_at: Option<f64>, quarantined: bool, now: f64) -> Option<f64> {
    if quarantined {
        return None;
    }
    let remaining = expires_at? - now;
    (remaining <= LOGIN_DUE_WITHIN_S).then_some(remaining.max(0.))
}

/// `"re-login in 5d 12h"` while a stored login is due, `"re-login needed"` once past, `None`
/// with nothing to say. `short` keeps the countdown's first unit (`"re-login in 5d"`).
pub fn format_login_expiry(
    expires_at: Option<f64>,
    quarantined: bool,
    now: f64,
    short: bool,
) -> Option<String> {
    let left = login_time_left(expires_at, quarantined, now)?;
    if left <= 0. {
        return Some("re-login needed".to_owned());
    }
    let countdown = format_countdown(left);
    let shown = if short {
        countdown.split(' ').next().unwrap_or_default()
    } else {
        &countdown
    };
    Some(format!("re-login in {shown}"))
}

/// The stamp as the local clock string the reset columns use.
pub fn login_expiry_clock(expires_at: f64, now: f64) -> String {
    match (utc_at(expires_at), utc_at(now)) {
        (Some(at), Some(now)) => reset_clock_string(at, now),
        _ => String::new(),
    }
}

/// Whether an OAuth token is expired or about to expire (within [`OAUTH_EXPIRY_BUFFER_MS`]).
/// A missing or non-numeric `expiresAt` is not expired.
pub fn is_oauth_token_expired(expires_at: Option<&Value>, now: f64) -> bool {
    let Some(expires_at) = expires_at.and_then(py_number) else {
        return false;
    };
    (now * 1000.).trunc() + OAUTH_EXPIRY_BUFFER_MS >= expires_at.trunc()
}

/// Whether an OAuth payload is an inference-only setup token: no refresh token and only the
/// `user:inference` scope. The usage endpoint needs `user:profile`, so it never answers one.
pub fn is_setup_token(oauth: Option<&Map<String, Value>>) -> bool {
    let Some(oauth) = oauth else {
        return false;
    };
    if truthy_at(oauth.get("refreshToken")) {
        return false;
    }
    match oauth.get("scopes") {
        Some(Value::Array(scopes)) => {
            !scopes.is_empty()
                && scopes
                    .iter()
                    .all(|scope| SETUP_TOKEN_SCOPES.contains(&scope.as_str().unwrap_or_default()))
        }
        _ => false,
    }
}

/// A short summary of a stored OAuth token's state, for logs: never the token.
pub fn build_token_status(credentials: &str, now: f64) -> Option<String> {
    let oauth = extract_oauth_data(credentials).filter(|data| !data.is_empty())?;
    let refresh = if truthy_at(oauth.get("refreshToken")) {
        "yes"
    } else {
        "no"
    };
    let unknown = || format!("oauth: unknown expiry, refresh token {refresh}");
    let Some(expires_at) = oauth.get("expiresAt").and_then(py_number) else {
        return Some(unknown());
    };
    let Some(expires) = utc_at(expires_at / 1000.) else {
        return Some(unknown());
    };
    let state = if is_oauth_token_expired(oauth.get("expiresAt"), now) {
        "expired"
    } else {
        "fresh"
    };
    let (countdown, clock) = format_reset(&expires.to_rfc3339(), now).ok()?;
    Some(format!(
        "oauth: {state}, refresh token {refresh}, expires {clock} in {countdown}"
    ))
}

/// A token as cswap puts it in `Bearer {token}`: a string as itself, another value as its text.
pub(crate) fn token_text(token: &Value) -> String {
    token
        .as_str()
        .map_or_else(|| token.to_string(), str::to_owned)
}

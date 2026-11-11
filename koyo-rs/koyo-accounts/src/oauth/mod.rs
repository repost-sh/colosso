//! Port of claude-swap's `oauth.py` (0fe22a8): OAuth tokens and the usage API of Claude Code
//! accounts. The token helpers and fingerprints are here; the refresh grant and the profile are
//! in `refresh.rs`, the usage endpoint, its results and its error kinds in `usage.rs`, a
//! response's unified rate-limit windows made a reading in `headers.rs`, the windows decisions
//! run on in `windows.rs`, reset strings in `reset.rs`, and the usage sentinels of
//! `json_output.py` in `sentinels.rs`.
//!
//! Requests go through `env.http` (the system `curl`, headers on stdin) with cswap's URLs,
//! headers, client id and timeouts. cswap's `User-Agent` is kept: the token endpoint answers 403
//! without one, and the usage endpoint's measured budget is per identity and User-Agent class
//! (`poll_policy`). Where cswap reads `datetime.now()`, the caller passes `now` (`env.now()`).
//!
//! Left out: the header probe that reads a setup-token's usage from a 1-token model request
//! (`request_usage_via_probe`, `_probe_request`, `_scoped_probe_limits`, `_usage_from_probe`,
//! `_unified_window_present`, `_header_reading_complete`; the windows' parsing it shares with
//! a chat reply's limits is in `headers.rs`), API-key paths, and the refresh inside a usage fetch
//! (`refresh_via`, `persist_credentials`, `_persist`, `fetch_usage_for_account`): Koyo never
//! refreshes a token to read usage (`docs/claude-accounts.md` 4), see
//! [`try_fetch_usage_for_account`]. Also left out, as nothing reads them: `build_token_status`
//! and the stored login's expiry (`login_expires_at_iso`, `login_expires_at_epoch`,
//! `login_time_left`, `format_login_expiry`); [`login_expiry_clock`] draws a stamp the app has.

mod headers;
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

pub use headers::{LiveLimits, LiveWindow, SEVEN_DAY_OI_MODEL};
pub use refresh::{
    AccountIdentity, CONSUME_BUSY_ERROR, DETERMINISTIC_REFRESH_ERRORS, INVALID_CLIENT_ERROR,
    INVALID_GRANT_ERROR, NO_REFRESH_TOKEN_ERROR, PROFILE_401_WARNING, REFRESH_TIMEOUT,
    RefreshOutcome, STASH_UNREADABLE_ERROR, STORE_UNMIRRORED_ERROR, TRANSIENT_ERROR,
    fetch_oauth_profile, parse_token_account, try_refresh_oauth_credentials,
};
pub use reset::{
    Iso, ResetError, format_countdown, format_reset, fresh_reset_strings, parse_iso,
    reset_clock_string,
};
pub(crate) use reset::{iso_timestamp, iso_utc, iso_z, utc_at};
pub use sentinels::*;
pub use usage::{
    BAD_RESPONSE_ERROR, FetchError, InvalidUsageResponse, NETWORK_ERROR, NO_ACCESS_TOKEN_ERROR,
    ORG_POLICY_ERROR, RATE_LIMITED_ERROR, RETRY_AFTER_BOUND_S, TIMEOUT_ERROR, TLS_CERT_ERROR,
    TOKEN_EXPIRED_ERROR, UNAUTHORIZED_ERROR, UsageOutcome, build_usage_result,
    classify_usage_error, log_usage_failure, parse_retry_after, reported_window,
    request_usage_data, try_fetch_usage_for_account, usage_failure_line, window_entry,
};
pub(crate) use windows::headroom_of;
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

/// `bytes` as lower-case hex, two digits each.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut hex, byte| {
            write!(hex, "{byte:02x}").ok();
            hex
        })
}

/// The sha256 of `text` as 64 hex digits.
pub(crate) fn sha256_hex(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
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

/// The stamp as the local clock string the reset columns use.
pub fn login_expiry_clock(expires_at: f64, now: f64) -> String {
    match (utc_at(expires_at), utc_at(now)) {
        (Some(at), Some(now)) => reset_clock_string(at, now),
        _ => String::new(),
    }
}

/// Whether an OAuth token is expired or about to expire (within [`OAUTH_EXPIRY_BUFFER_MS`]).
/// A missing or non-numeric `expiresAt` is not expired. Both sides are whole milliseconds, as
/// cswap's `int()`s make them.
pub fn is_oauth_token_expired(expires_at: Option<&Value>, now: f64) -> bool {
    expires_within(
        expires_at.and_then(py_number).map(f64::trunc),
        (now * 1000.).trunc(),
        OAUTH_EXPIRY_BUFFER_MS,
    )
}

/// Whether a token expiring at `expires_at` (epoch ms) is expired or expires within
/// `buffer_ms` of `now_ms`; an unknown expiry isn't.
pub(crate) fn expires_within(expires_at: Option<f64>, now_ms: f64, buffer_ms: f64) -> bool {
    expires_at.is_some_and(|expires_at| now_ms + buffer_ms >= expires_at)
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

/// Whether an OAuth payload holds a login: a non-empty access or refresh token. Claude Code
/// empties both in place after a rejected refresh, keeping the wrapper.
pub fn has_login_token(oauth: &Map<String, Value>) -> bool {
    truthy_at(oauth.get("accessToken")) || truthy_at(oauth.get("refreshToken"))
}

/// Whether an OAuth payload holds a full token pair: a non-empty access and refresh token. A
/// wiped login and an access-only blob are not one, and writing either over a backup would
/// replace the slot's only refresh token.
pub fn has_full_token_pair(oauth: &Map<String, Value>) -> bool {
    truthy_at(oauth.get("accessToken")) && truthy_at(oauth.get("refreshToken"))
}

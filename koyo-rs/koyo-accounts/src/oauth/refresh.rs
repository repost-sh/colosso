//! Part of the port of claude-swap's `oauth.py` (0fe22a8): the refresh-token grant
//! (`RefreshOutcome`, `try_refresh_oauth_credentials`, `_parse_token_account`) and the profile
//! that names whose token is in hand (`fetch_oauth_profile`). cswap's `refresh_oauth_credentials`
//! wrapper comes along. Neither is called to read usage (see `usage.rs`); the switcher's refresh
//! gate calls the refresh, and nothing here holds a lock while it waits on the network.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::value::{py_number, truthy_at};
use super::{OAUTH_CLIENT_ID, OAUTH_TOKEN_URL, PROFILE_URL, USER_AGENT};
use crate::Env;
use crate::http::Request;

/// Refresh failures that won't resolve by retrying this pass, so a caller must not fall through
/// to the usage endpoint with the known-expired token. `consume-busy` is among them: the retry
/// re-enters the same gate, finds it held, and spends a guaranteed 401 to learn nothing.
pub const DETERMINISTIC_REFRESH_ERRORS: [&str; 4] = [
    "store-unmirrored",
    "invalid_client",
    "consume-busy",
    "stash-unreadable",
];

/// cswap's refresh budget (`timeout_s=10.0`).
pub const REFRESH_TIMEOUT: Duration = Duration::from_secs(10);
/// The profile is one bounded call: it may add latency to a switch, never hang it.
pub const PROFILE_TIMEOUT: Duration = Duration::from_secs(5);

/// What a profile 401 logs: evidence, not proof, that the live token can't authenticate.
pub const PROFILE_401_WARNING: &str = "OAuth profile returned 401 while resolving credential \
     ownership; proceeding without identity (pre-fix behavior).";

/// Whose token it is: the profile's answer, or the identity a refresh grant may carry. A
/// non-empty `uuid` is required; the others are strings or nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIdentity {
    pub uuid: String,
    pub email: Option<String>,
    pub organization_uuid: Option<String>,
}

/// The result of a refresh-token grant.
///
/// `credentials` is the whole rotated credentials JSON on success. `error` tells a dead
/// lineage (permanent: quarantine, stop retrying) from a blip (transient: retry later):
///
/// - `None`: success.
/// - `"invalid_grant"`: the token endpoint rejected the grant; re-login is required.
/// - `"invalid_client"`: our client id was rejected: systemic, no evidence about the slot.
/// - `"no_refresh_token"`: the credential carries no usable refresh token (permanent too).
/// - `"transient"`: network or server; the token may still be good.
///
/// The switcher's consume gate adds its own kinds (`DETERMINISTIC_REFRESH_ERRORS`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefreshOutcome {
    pub credentials: Option<String>,
    pub error: Option<String>,
    /// The identity the token endpoint may send beside a grant: opportunistic, never relied on.
    pub token_account: Option<AccountIdentity>,
    /// The fingerprint of the generation actually sent, set by the consume gate, which may
    /// substitute a fresher re-read for the caller's snapshot: strikes follow the bytes sent.
    pub consumed_fp: Option<String>,
    /// Whether the consumed successor reached the stash; meaningful only on the gate's demoted
    /// outcome (`transient` with credentials).
    pub stashed: bool,
}

impl RefreshOutcome {
    pub fn failed(error: &str) -> RefreshOutcome {
        RefreshOutcome {
            error: Some(error.to_owned()),
            ..RefreshOutcome::default()
        }
    }
}

/// Refreshes an OAuth access token by a direct POST to the token endpoint, within `timeout`.
///
/// `no_refresh_token` is permanent (it strikes at once), so it needs a whole OAuth object that
/// truly lacks the field; an unparseable or non-object blob is more likely a torn read and is
/// `transient`. A 4xx is permanent only when the body's top-level `error` (RFC 6749 §5.2) says
/// `invalid_grant` or `invalid_client`: a misread transient costs one retry, a misread permanent
/// would quarantine a live token.
pub fn try_refresh_oauth_credentials(
    env: &Env,
    credentials: &str,
    timeout: Duration,
) -> RefreshOutcome {
    let Ok(Value::Object(mut data)) = serde_json::from_str::<Value>(credentials) else {
        return RefreshOutcome::failed("transient");
    };
    let mut oauth = match data.get("claudeAiOauth") {
        Some(Value::Object(oauth)) if truthy_at(oauth.get("refreshToken")) => oauth.clone(),
        _ => return RefreshOutcome::failed("no_refresh_token"),
    };
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": oauth.get("refreshToken"),
        "client_id": OAUTH_CLIENT_ID,
    });
    let request = Request::post(OAUTH_TOKEN_URL, body.to_string(), timeout)
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT);
    let response = match env.http.send(&request) {
        Ok(response) => response,
        Err(error) => {
            log::debug!("OAuth refresh failed: {error}");
            return RefreshOutcome::failed("transient");
        }
    };
    if !(200..300).contains(&response.status) {
        let text = String::from_utf8_lossy(&response.body);
        let shown: String = text.chars().take(500).collect();
        log::debug!(
            "OAuth refresh failed: HTTP {}, body: {shown}",
            response.status
        );
        if matches!(response.status, 400 | 401 | 403) {
            let error = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|body| body.get("error").and_then(Value::as_str).map(str::to_owned));
            if let Some(error) = error.filter(|e| e == "invalid_grant" || e == "invalid_client") {
                return RefreshOutcome::failed(&error);
            }
        }
        return RefreshOutcome::failed("transient");
    }
    let Some(granted) = rotate(&mut oauth, &response.body, env.now()) else {
        log::debug!("OAuth refresh failed: unreadable grant");
        return RefreshOutcome::failed("transient");
    };
    data.insert("claudeAiOauth".to_owned(), Value::Object(oauth));
    RefreshOutcome {
        credentials: Some(Value::Object(data).to_string()),
        token_account: parse_token_account(&granted),
        ..RefreshOutcome::default()
    }
}

/// Writes a grant's answer into `oauth`: the access token, `expiresAt` from `expires_in`, the
/// refresh token and scopes when sent. `None` for an answer cswap's would raise on.
fn rotate(oauth: &mut Map<String, Value>, body: &[u8], now: f64) -> Option<Map<String, Value>> {
    let Value::Object(granted) = serde_json::from_slice::<Value>(body).ok()? else {
        return None;
    };
    let access = granted.get("access_token")?.clone();
    let expires_in = granted.get("expires_in")?;
    let now_ms = (now * 1000.).trunc();
    let expires_at = match expires_in.as_i64() {
        Some(seconds) => Value::from(now_ms as i64 + seconds * 1000),
        None => Value::from(now_ms + py_number(expires_in)? * 1000.),
    };
    let scopes = match granted.get("scope").filter(|scope| truthy_at(Some(scope))) {
        Some(scope) => Some(
            scope
                .as_str()?
                .split_whitespace()
                .map(|s| Value::from(s.to_owned()))
                .collect::<Vec<_>>(),
        ),
        None => None,
    };
    oauth.insert("accessToken".to_owned(), access);
    oauth.insert("expiresAt".to_owned(), expires_at);
    if let Some(refresh) = granted
        .get("refresh_token")
        .filter(|token| truthy_at(Some(token)))
    {
        oauth.insert("refreshToken".to_owned(), refresh.clone());
    }
    if let Some(scopes) = scopes {
        oauth.insert("scopes".to_owned(), Value::Array(scopes));
    }
    Some(granted)
}

/// The identity a token-endpoint answer may carry (`account` / `organization`), as Claude Code
/// surfaces it as `tokenAccount`. The same strict boundary as [`fetch_oauth_profile`]: a
/// non-empty string `account.uuid`, trimmed; optional fields strings or nothing; anything
/// malformed is `None`, and never breaks the refresh that carried it.
pub fn parse_token_account(granted: &Map<String, Value>) -> Option<AccountIdentity> {
    identity(granted, "email_address")
}

/// `{uuid, email, organizationUuid}` from an answer's `account` and `organization`, reading the
/// email from `account[email_key]`.
fn identity(data: &Map<String, Value>, email_key: &str) -> Option<AccountIdentity> {
    let account = data.get("account")?.as_object()?;
    let uuid = account.get("uuid")?.as_str()?.trim();
    if uuid.is_empty() {
        return None;
    }
    let organization_uuid = data
        .get("organization")
        .and_then(Value::as_object)
        .and_then(|organization| organization.get("uuid"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(AccountIdentity {
        uuid: uuid.to_owned(),
        email: account
            .get(email_key)
            .and_then(Value::as_str)
            .map(str::to_owned),
        organization_uuid,
    })
}

/// Refreshes an OAuth access token; `None` on any failure (see [`RefreshOutcome`]).
pub fn refresh_oauth_credentials(env: &Env, credentials: &str) -> Option<String> {
    try_refresh_oauth_credentials(env, credentials, REFRESH_TIMEOUT).credentials
}

/// Resolves an access token to its account identity with `GET /api/oauth/profile`, or `None`.
///
/// It answers the one question the credential bytes can't: whose token is this. Callers take
/// `None` as "unresolvable", never as an error: the identity check is advisory, so the boundary
/// is strict the other way, and an answer counts only with a non-empty string `account.uuid`
/// (a schema change that renames it stays on the fail-open path). `email` and
/// `organizationUuid` are optional. Never called with a credential or config lock held.
pub fn fetch_oauth_profile(env: &Env, access_token: &str) -> Option<AccountIdentity> {
    let request = Request::get(PROFILE_URL, PROFILE_TIMEOUT)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT);
    let response = match env.http.send(&request) {
        Ok(response) => response,
        Err(error) => {
            log::debug!("OAuth profile fetch failed: {error}");
            return None;
        }
    };
    if !(200..300).contains(&response.status) {
        if response.status == 401 {
            // Evidence, not proof: this also fires benignly (the family rotated, then the
            // access token expired on an idle machine). Log only; the caller goes on without.
            log::warn!("{PROFILE_401_WARNING}");
        } else {
            log::debug!("OAuth profile fetch failed: HTTP {}", response.status);
        }
        return None;
    }
    let Ok(Value::Object(data)) = serde_json::from_slice::<Value>(&response.body) else {
        log::debug!("OAuth profile fetch failed: unreadable answer");
        return None;
    };
    let found = identity(&data, "email");
    if found.is_none() {
        log::debug!("OAuth profile response missing account.uuid");
    }
    found
}

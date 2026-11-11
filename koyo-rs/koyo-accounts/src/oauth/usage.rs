//! Part of the port of claude-swap's `oauth.py` (0fe22a8): the usage endpoint
//! (`request_usage_data`), its answer normalised (`build_usage_result`, `_reported_window`,
//! `_window_entry`, per-model `limits[]`), the error kinds (`_classify_usage_error`), the failure
//! log line (`_log_usage_failure`) and one account's fetch (`UsageOutcome`,
//! `try_fetch_usage_for_account`, `fetch_usage`).
//!
//! Two rules differ from cswap's, as `docs/claude-accounts.md` 4 decides:
//!
//! - Never a token refresh to read usage, as Orca; renewal happens only through the refresh
//!   gate. cswap refreshes an inactive account's expired token before the request and retries
//!   a 401 after a refresh; here an access token already expired by `expiresAt` (cswap's own
//!   [`super::is_oauth_token_expired`], whatever the account) sends no request and returns
//!   [`TOKEN_EXPIRED_ERROR`], and a 401 is returned as its kind, `http-401`.
//! - `Retry-After` is read as a bounded integer ([`parse_retry_after`]): cswap's `float()` took
//!   `inf` and `1e400`, which only the store's park bound kept from wedging a row.

use std::time::Duration;

use serde_json::{Map, Value};

use super::reset::format_reset_value;
use super::value::{py_float, py_number, truthy, truthy_at};
use super::{OAUTH_BETA_HEADER, USAGE_URL, USER_AGENT, extract_oauth_data, is_setup_token};
use crate::Env;
use crate::http::{HttpError, Request, Response};

/// The usage request's bound (cswap's `timeout=5`).
pub const USAGE_TIMEOUT: Duration = Duration::from_secs(5);

/// The kind a fetch returns for an access token already expired, sending nothing; the switcher
/// shows it as [`super::USAGE_TOKEN_EXPIRED`].
pub const TOKEN_EXPIRED_ERROR: &str = "token-expired";

/// The org-policy 403 (a lapsed plan), the one status whose body names its cause.
pub const ORG_POLICY_ERROR: &str = "oauth_not_allowed_for_organization";
/// The other kinds a failed reading is named by, as `_classify_usage_error` names them; a status
/// that isn't one of these is `http-<status>`.
pub const RATE_LIMITED_ERROR: &str = "http-429";
pub const TIMEOUT_ERROR: &str = "timeout";
pub const TLS_CERT_ERROR: &str = "tls-cert";
pub const NETWORK_ERROR: &str = "network";
pub const BAD_RESPONSE_ERROR: &str = "bad-response";

/// The most a `Retry-After` is read as: a day, longer than any wait anything here honours (the
/// store parks a row at most 4,500 s, a plan read waits at most 24 h).
pub const RETRY_AFTER_BOUND_S: u64 = 86_400;

/// A reported window that can't be read as a measurement (`bad-response`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidUsageResponse(pub String);

/// Why a usage fetch got no reading: cswap's exceptions, by what `_classify_usage_error` reads
/// of them. Shown, it names the status or the transport's failure, never a body or a header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// `HTTPError`: an answer outside 2xx, with its headers and body.
    Status(Response),
    /// No answer: timeout, refused certificate, no route.
    Transport(HttpError),
    /// `JSONDecodeError` or `_InvalidUsageResponse`.
    BadResponse(String),
    /// Any other exception, by its type name (`ValueError`, `TypeError`, ...).
    Other(&'static str),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Status(response) => write!(f, "HTTP {}", response.status),
            FetchError::Transport(error) => write!(f, "{error}"),
            FetchError::BadResponse(why) => write!(f, "bad response: {why}"),
            FetchError::Other(name) => f.write_str(name),
        }
    }
}

impl From<InvalidUsageResponse> for FetchError {
    fn from(invalid: InvalidUsageResponse) -> FetchError {
        FetchError::BadResponse(invalid.0)
    }
}

/// The result of one usage fetch: `usage` on success (possibly `None`, an answer with no
/// window), else `error`, a [`classify_usage_error`] kind or one of the pre-request kinds
/// (`no-access-token`, `setup-token`, [`TOKEN_EXPIRED_ERROR`]), with the server's `Retry-After`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageOutcome {
    pub usage: Option<Value>,
    pub error: Option<String>,
    pub retry_after_s: Option<f64>,
    /// The fingerprint of the credential whose refresh token was sent, on a permanent auth
    /// error, so the store binds the strike to that generation. Set by the switcher's gate.
    pub struck_fp: Option<String>,
}

impl UsageOutcome {
    pub fn read(usage: Option<Value>) -> UsageOutcome {
        UsageOutcome {
            usage,
            ..UsageOutcome::default()
        }
    }

    pub fn failed(kind: &str, retry_after_s: Option<f64>) -> UsageOutcome {
        UsageOutcome {
            error: Some(kind.to_owned()),
            retry_after_s,
            ..UsageOutcome::default()
        }
    }
}

/// The raw utilization answer of the usage endpoint.
///
/// # Errors
/// The status, the transport's failure, or an unreadable body.
pub fn request_usage_data(env: &Env, access_token: &str) -> Result<Value, FetchError> {
    let request = Request::get(USAGE_URL, USAGE_TIMEOUT)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("anthropic-beta", OAUTH_BETA_HEADER)
        .header("User-Agent", USER_AGENT);
    let response = env.http.send(&request).map_err(FetchError::Transport)?;
    if !(200..300).contains(&response.status) {
        return Err(FetchError::Status(response));
    }
    let text = std::str::from_utf8(&response.body).map_err(|error| {
        log::debug!("Usage answer isn't UTF-8: {error}");
        FetchError::Other("UnicodeDecodeError")
    })?;
    serde_json::from_str(text).map_err(|error| FetchError::BadResponse(error.to_string()))
}

/// The raw `five_hour`/`seven_day` window, or `None` when absent.
///
/// Absent is missing or `null`: a limit the account doesn't have (a verified Ultra account
/// reports `five_hour: null`). `{}` is absent too, a compatibility exception for the falsy
/// check this replaces. Any other non-object is a malformed answer.
///
/// # Errors
/// [`InvalidUsageResponse`] for a window that isn't an object or `null`.
pub fn reported_window<'a>(
    data: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a Map<String, Value>>, InvalidUsageResponse> {
    match data.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(window)) => Ok((!window.is_empty()).then_some(window)),
        Some(_) => Err(InvalidUsageResponse(format!(
            "{key} must be an object or null"
        ))),
    }
}

/// One reported window normalised to `{"pct", "resets_at", "countdown", "clock"}`.
///
/// `utilization: null` (Claude Code's usage schema allows it) keeps the window with `pct` null,
/// unknown rather than zero, and any `resets_at`. A missing, boolean, string, non-finite or
/// negative `utilization` fails the whole reading: dropping the window would let the other one
/// pass as a complete reading.
///
/// # Errors
/// [`FetchError::BadResponse`] for an unusable `utilization`, or the reset's [`FetchError::Other`].
pub fn window_entry(key: &str, window: &Map<String, Value>, now: f64) -> Result<Value, FetchError> {
    let Some(pct) = window.get("utilization") else {
        return Err(InvalidUsageResponse(format!("{key}.utilization is missing")).into());
    };
    let valid =
        pct.is_null() || super::value::number(pct).is_some_and(|pct| pct.is_finite() && pct >= 0.);
    if !valid {
        return Err(InvalidUsageResponse(format!(
            "{key}.utilization must be null or a finite non-negative number"
        ))
        .into());
    }
    let mut entry = Map::new();
    entry.insert("pct".to_owned(), pct.clone());
    if let Some(resets_at) = window.get("resets_at").filter(|v| truthy(v)) {
        with_reset(&mut entry, resets_at, now)?;
    }
    Ok(Value::Object(entry))
}

/// `resets_at` and its fetch-time `countdown` and `clock` into `entry`.
fn with_reset(
    entry: &mut Map<String, Value>,
    resets_at: &Value,
    now: f64,
) -> Result<(), FetchError> {
    let (countdown, clock) =
        format_reset_value(resets_at, now).map_err(|error| FetchError::Other(error.kind()))?;
    entry.insert("resets_at".to_owned(), resets_at.clone());
    entry.insert("countdown".to_owned(), Value::from(countdown));
    entry.insert("clock".to_owned(), Value::from(clock));
    Ok(())
}

/// The usage endpoint's answer normalised to the shape everything else reads: `five_hour` and
/// `seven_day` through [`reported_window`] and [`window_entry`], `spend` from `extra_usage`,
/// and `scoped` from the `weekly_scoped` entries of `limits[]`. `None` with nothing in it.
///
/// # Errors
/// A malformed window fails the whole fetch, as `bad-response`.
pub fn build_usage_result(data: &Value, now: f64) -> Result<Option<Value>, FetchError> {
    let data = match data {
        Value::Object(data) => data,
        Value::Array(_) | Value::String(_) => return Err(FetchError::Other("AttributeError")),
        _ => return Err(FetchError::Other("TypeError")),
    };
    log::debug!(
        "Usage API response: {}",
        serde_json::to_string_pretty(data).unwrap_or_default()
    );
    let mut result = Map::new();
    for key in ["five_hour", "seven_day"] {
        if let Some(window) = reported_window(data, key)? {
            result.insert(key.to_owned(), window_entry(key, window, now)?);
        }
    }
    if let Some(spend) = spend_entry(data.get("extra_usage"), now)? {
        result.insert("spend".to_owned(), spend);
    }
    // Per-model weekly limits live in `limits[]` as `weekly_scoped` entries with a
    // `scope.model.display_name` ("Fable"); the legacy keys never carry them.
    if let Some(Value::Array(limits)) = data.get("limits") {
        let mut scoped = Vec::new();
        for limit in limits.iter().filter_map(Value::as_object) {
            let name = limit
                .get("scope")
                .and_then(Value::as_object)
                .and_then(|scope| scope.get("model"))
                .and_then(Value::as_object)
                .and_then(|model| model.get("display_name"))
                .filter(|name| truthy(name));
            let pct = limit.get("percent").and_then(py_number);
            let (Some(name), Some(pct)) = (name, pct) else {
                continue;
            };
            let mut entry = Map::new();
            entry.insert("name".to_owned(), name.clone());
            entry.insert("pct".to_owned(), Value::from(pct));
            if let Some(resets_at) = limit.get("resets_at").filter(|v| truthy(v)) {
                with_reset(&mut entry, resets_at, now)?;
            }
            scoped.push(Value::Object(entry));
        }
        if !scoped.is_empty() {
            result.insert("scoped".to_owned(), Value::Array(scoped));
        }
    }
    Ok((!result.is_empty()).then_some(Value::Object(result)))
}

/// The pay-as-you-go spend line from `extra_usage`, when enabled and complete.
///
/// Claude Code returns nullable `used_credits`, `monthly_limit` (null is unlimited) and
/// `utilization`; the line needs all three, so a null skips just the spend entry.
fn spend_entry(extra: Option<&Value>, now: f64) -> Result<Option<Value>, FetchError> {
    let Some(extra) = extra.filter(|v| truthy(v)) else {
        return Ok(None);
    };
    let Some(extra) = extra.as_object() else {
        return Err(FetchError::Other("AttributeError"));
    };
    if !truthy_at(extra.get("is_enabled")) {
        return Ok(None);
    }
    let field = |key: &str| extra.get(key).filter(|v| !v.is_null());
    let (Some(used), Some(limit), Some(utilization)) = (
        field("used_credits"),
        field("monthly_limit"),
        field("utilization"),
    ) else {
        return Ok(None);
    };
    let parsed = (py_float(used), py_float(limit), py_float(utilization));
    let (Some(used), Some(limit), Some(pct)) = parsed else {
        log::debug!("extra_usage parse failed: a field isn't a number");
        return Ok(None);
    };
    let mut entry = Map::new();
    entry.insert("used".to_owned(), Value::from(used / 100.));
    entry.insert("limit".to_owned(), Value::from(limit / 100.));
    entry.insert("pct".to_owned(), Value::from(pct));
    entry.insert(
        "currency".to_owned(),
        extra
            .get("currency")
            .cloned()
            .unwrap_or_else(|| Value::from("USD")),
    );
    if let Some(resets_at) = extra.get("resets_at").filter(|v| truthy(v))
        && with_reset(&mut entry, resets_at, now).is_err()
    {
        log::debug!("extra_usage parse failed: unreadable resets_at");
        return Ok(None);
    }
    Ok(Some(Value::Object(entry)))
}

/// `Retry-After` in seconds, as a bounded integer: digits, an optional sign, a negative ask
/// read as 0 and anything over [`RETRY_AFTER_BOUND_S`] as the bound. The HTTP-date form, a
/// fraction, `inf` and the like are `None`, as no header.
pub fn parse_retry_after(raw: &str) -> Option<f64> {
    let text = raw.trim();
    let (negative, digits) = match text.split_at_checked(1) {
        Some(("-", rest)) => (true, rest),
        Some(("+", rest)) => (false, rest),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if negative {
        return Some(0.);
    }
    let digits = digits.trim_start_matches('0');
    let seconds = if digits.len() > 6 {
        RETRY_AFTER_BOUND_S
    } else {
        digits.parse::<u64>().unwrap_or(0).min(RETRY_AFTER_BOUND_S)
    };
    Some(seconds as f64)
}

/// `(kind, retry_after_s)` of a failed fetch: a short stable token for logs and backoff
/// (`http-429`, `timeout`, `tls-cert`, `network`, `bad-response`, [`ORG_POLICY_ERROR`] for a
/// 403 whose body says so, or the exception's name), and the server's `Retry-After`.
///
/// A certificate the client refused is not an unreachable host: its repair is trust or the
/// clock, so it has its own kind. curl's TLS failures (`HttpError::Tls`) are all read so.
pub fn classify_usage_error(error: &FetchError) -> (String, Option<f64>) {
    match error {
        FetchError::Status(response) => {
            let retry_after = response
                .header("Retry-After")
                .filter(|raw| !raw.is_empty())
                .and_then(parse_retry_after);
            if response.status == 403 && org_policy_refusal(&response.body) {
                return (ORG_POLICY_ERROR.to_owned(), retry_after);
            }
            (format!("http-{}", response.status), retry_after)
        }
        FetchError::Transport(HttpError::Timeout) => (TIMEOUT_ERROR.to_owned(), None),
        FetchError::Transport(HttpError::Tls(_)) => (TLS_CERT_ERROR.to_owned(), None),
        FetchError::Transport(HttpError::Network(_)) => (NETWORK_ERROR.to_owned(), None),
        FetchError::BadResponse(_) => (BAD_RESPONSE_ERROR.to_owned(), None),
        FetchError::Other(name) => ((*name).to_owned(), None),
    }
}

/// Whether a 403's body (its first 4 KiB, as cswap reads) is the org-policy refusal.
fn org_policy_refusal(body: &[u8]) -> bool {
    let head = body.get(..body.len().min(4096)).unwrap_or_default();
    serde_json::from_slice::<Value>(head)
        .ok()
        .and_then(|body| {
            body.get("error")?
                .get("details")?
                .get("error_code")?
                .as_str()
                .map(|code| code == ORG_POLICY_ERROR)
        })
        .unwrap_or(false)
}

/// The warning a failed fetch logs, the line people paste into public issues: so `context`
/// never carries the email, and the server's `Retry-After` rides along.
pub fn usage_failure_line(context: &str, kind: &str, retry_after_s: Option<f64>) -> String {
    let at = if context.is_empty() {
        String::new()
    } else {
        format!(" {context}")
    };
    let mut cause = match retry_after_s {
        Some(seconds) => format!("{kind}, retry-after {seconds:.0}s"),
        None => kind.to_owned(),
    };
    if kind == RATE_LIMITED_ERROR {
        // Per token or per account depends on the org's 429 regime (both measured, see
        // `poll_policy`), so the line stays scope-neutral.
        cause.push_str(" (usage-endpoint budget reached; backing off)");
    }
    format!("Usage fetch failed{at}: {cause}")
}

/// One WARNING line with the cause (issue #85 was undiagnosable with failures at DEBUG); the
/// failure itself stays at DEBUG.
pub fn log_usage_failure(
    context: &str,
    error: &FetchError,
    kind: &str,
    retry_after_s: Option<f64>,
) {
    log::warn!("{}", usage_failure_line(context, kind, retry_after_s));
    let at = if context.is_empty() { "" } else { " " };
    log::debug!("Usage fetch failure detail{at}{context}: {error}");
}

/// The 5-hour and 7-day utilization of an access token, or `None` (the failure is logged).
pub fn fetch_usage(env: &Env, access_token: &str) -> Option<Value> {
    match request_usage_data(env, access_token)
        .and_then(|data| build_usage_result(&data, env.now()))
    {
        Ok(usage) => usage,
        Err(error) => {
            let (kind, _) = classify_usage_error(&error);
            log_usage_failure("", &error, &kind, None);
            None
        }
    }
}

/// One account's usage, with no refresh (the module says why): no access token is
/// `no-access-token`; a setup token is `setup-token`, as the usage endpoint can't answer its
/// scope; an access token already expired is [`TOKEN_EXPIRED_ERROR`]; each sends no request.
/// Otherwise the endpoint is asked once and its answer, or its failure's kind, returned.
pub fn try_fetch_usage_for_account(
    env: &Env,
    account_num: &str,
    credentials: &str,
) -> UsageOutcome {
    // No email: the context is paste-safe for public issues.
    let context = format!("for account {account_num}");
    let oauth = extract_oauth_data(credentials).filter(|oauth| !oauth.is_empty());
    let Some(access_token) = oauth
        .as_ref()
        .and_then(|oauth| oauth.get("accessToken"))
        .filter(|token| truthy(token))
    else {
        return UsageOutcome::failed("no-access-token", None);
    };
    if is_setup_token(oauth.as_ref()) {
        return UsageOutcome::failed("setup-token", None);
    }
    let expires_at = oauth.as_ref().and_then(|oauth| oauth.get("expiresAt"));
    if super::is_oauth_token_expired(expires_at, env.now()) {
        return UsageOutcome::failed(TOKEN_EXPIRED_ERROR, None);
    }
    let token = super::token_text(access_token);
    match request_usage_data(env, &token).and_then(|data| build_usage_result(&data, env.now())) {
        Ok(usage) => UsageOutcome::read(usage),
        Err(error) => {
            let (kind, retry_after) = classify_usage_error(&error);
            log_usage_failure(&context, &error, &kind, retry_after);
            UsageOutcome::failed(&kind, retry_after)
        }
    }
}

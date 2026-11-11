//! From claude-swap's `tests/test_oauth.py` (0fe22a8):
//! `TestFetchUsage`, `TestFetchUsageForAccount` (to the spec's no-refresh rule),
//! `TestClassifyUsageError`, `TestTryFetchUsageOutcome`, and the two `TestSetupTokenHeaderProbe`
//! cases that hold without the probe.

use serde_json::{Value, json};

use super::{answer, fetch_usage, fixed_now, header, num, sandbox_at};
use crate::http::{HttpError, Response};
use crate::oauth::*;
use crate::testing::{Sandbox, iso};

fn fetch_with_response(payload: &Value) -> Option<Value> {
    let sandbox = sandbox_at(fixed_now());
    sandbox.http.queue_json(USAGE_URL, 200, payload);
    fetch_usage(&sandbox.env(), "sk-test-token")
}

// TestFetchUsage

#[test]
fn success() {
    let future = iso(fixed_now() + 3600.);
    let result = fetch_with_response(&json!({
        "five_hour": {"utilization": 22.0, "resets_at": future},
        "seven_day": {"utilization": 61.0, "resets_at": future},
    }))
    .unwrap();
    assert_eq!(num(&result, &["five_hour", "pct"]), Some(22.));
    assert_eq!(num(&result, &["seven_day", "pct"]), Some(61.));
    assert_eq!(result["five_hour"]["countdown"], "1h 0m");
}

#[test]
fn network_error() {
    let sandbox = Sandbox::new();
    sandbox.http.queue(USAGE_URL, Err(HttpError::Timeout));
    assert_eq!(fetch_usage(&sandbox.env(), "sk-test-token"), None);
}

#[test]
fn http_error_logs_in_debug_mode() {
    let sandbox = Sandbox::new();
    sandbox.http.queue(USAGE_URL, Ok(answer(429, &[], "")));
    assert_eq!(fetch_usage(&sandbox.env(), "sk-test-token"), None);
    // What the warning and its debug detail say.
    assert!(usage_failure_line("", "http-429", None).starts_with("Usage fetch failed"));
    let error = FetchError::Status(Response {
        status: 429,
        headers: Vec::new(),
        body: b"secret body".to_vec(),
    });
    assert_eq!(error.to_string(), "HTTP 429");
}

#[test]
fn bad_response() {
    assert_eq!(fetch_with_response(&json!({})), None);
}

#[test]
fn null_resets_at() {
    let future = iso(fixed_now() + 22. * 3600.);
    let result = fetch_with_response(&json!({
        "five_hour": {"utilization": 0.0, "resets_at": null},
        "seven_day": {"utilization": 100.0, "resets_at": future},
    }))
    .unwrap();
    assert_eq!(num(&result, &["five_hour", "pct"]), Some(0.));
    assert!(result["five_hour"].get("clock").is_none());
    assert!(result["five_hour"].get("countdown").is_none());
    assert_eq!(num(&result, &["seven_day", "pct"]), Some(100.));
    assert!(result["seven_day"].get("clock").is_some());
    assert!(result["seven_day"].get("countdown").is_some());
}

fn windows_and(extra: &Value) -> Value {
    let mut payload = json!({
        "five_hour": {"utilization": 22.0, "resets_at": null},
        "seven_day": {"utilization": 61.0, "resets_at": null},
    });
    for (key, value) in extra.as_object().unwrap() {
        payload[key] = value.clone();
    }
    payload
}

#[test]
fn extra_usage_complete() {
    let result = fetch_with_response(&windows_and(&json!({"extra_usage": {
        "is_enabled": true, "used_credits": 72900, "monthly_limit": 500_000,
        "utilization": 14.58, "currency": "USD",
    }})))
    .unwrap();
    assert_eq!(num(&result, &["five_hour", "pct"]), Some(22.));
    assert_eq!(num(&result, &["seven_day", "pct"]), Some(61.));
    assert_eq!(num(&result, &["spend", "used"]), Some(729.));
    assert_eq!(num(&result, &["spend", "limit"]), Some(5000.));
    assert_eq!(num(&result, &["spend", "pct"]), Some(14.58));
    assert_eq!(result["spend"]["currency"], "USD");
}

#[test]
fn extra_usage_unlimited_keeps_other_rows() {
    let result = fetch_with_response(&windows_and(&json!({"extra_usage": {
        "is_enabled": true, "used_credits": 72900, "monthly_limit": null,
        "utilization": null, "currency": "USD",
    }})))
    .unwrap();
    assert_eq!(num(&result, &["five_hour", "pct"]), Some(22.));
    assert_eq!(num(&result, &["seven_day", "pct"]), Some(61.));
    assert!(result.get("spend").is_none());
}

#[test]
fn extra_usage_partial_keeps_other_rows() {
    let result = fetch_with_response(&windows_and(&json!({"extra_usage": {
        "is_enabled": true, "used_credits": null, "monthly_limit": 500_000, "utilization": 14.58,
    }})))
    .unwrap();
    assert_eq!(num(&result, &["five_hour", "pct"]), Some(22.));
    assert!(result.get("spend").is_none());
}

#[test]
fn extra_usage_disabled_keeps_other_rows() {
    let result = fetch_with_response(&windows_and(&json!({"extra_usage": {
        "is_enabled": false, "used_credits": 72900, "monthly_limit": 500_000, "utilization": 14.58,
    }})))
    .unwrap();
    assert_eq!(num(&result, &["seven_day", "pct"]), Some(61.));
    assert!(result.get("spend").is_none());
}

#[test]
fn scoped_per_model_limits() {
    let future = iso(fixed_now() + 3. * 3600.);
    let result = fetch_with_response(&json!({
        "five_hour": {"utilization": 7.0, "resets_at": null},
        "seven_day": {"utilization": 72.0, "resets_at": null},
        "seven_day_opus": null,
        "limits": [
            {"kind": "session", "group": "session", "percent": 7,
             "resets_at": null, "scope": null, "is_active": false},
            {"kind": "weekly_all", "group": "weekly", "percent": 72,
             "resets_at": null, "scope": null, "is_active": false},
            {"kind": "weekly_scoped", "group": "weekly", "percent": 100,
             "severity": "critical", "resets_at": future,
             "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null},
             "is_active": true},
        ],
    }))
    .unwrap();
    // Only the model-scoped entry is surfaced; session/weekly_all (scope null) are not.
    let scoped = result["scoped"].as_array().unwrap();
    assert_eq!(scoped.len(), 1);
    let fable = &scoped[0];
    assert_eq!(fable["name"], "Fable");
    assert_eq!(fable["pct"].as_f64(), Some(100.));
    assert_eq!(fable["resets_at"], future.as_str());
    assert_eq!(fable["countdown"], "3h 0m");
    assert!(fable.get("clock").is_some());
}

#[test]
fn no_limits_no_scoped_key() {
    let result = fetch_with_response(&windows_and(&json!({}))).unwrap();
    assert!(result.get("scoped").is_none());
}

// TestFetchUsageForAccount, under the spec's rule: no refresh to read usage.

fn account_credentials(sandbox: &Sandbox, expires_at_ms: Option<f64>) -> String {
    let now_ms = sandbox.env().now() * 1000.;
    json!({
        "claudeAiOauth": {
            "accessToken": "old-access",
            "refreshToken": "old-refresh",
            "expiresAt": expires_at_ms.unwrap_or(now_ms + 3_600_000.) as i64,
            "scopes": ["user:profile", "user:inference", "user:sessions:claude_code"],
            "subscriptionType": "pro",
            "rateLimitTier": "default_claude_ai",
        },
        "organizationUuid": "org-1",
    })
    .to_string()
}

fn usage_response(h5: f64, d7: f64) -> Value {
    json!({
        "five_hour": {"utilization": h5, "resets_at": null},
        "seven_day": {"utilization": d7, "resets_at": null},
    })
}

/// cswap's `test_refreshes_expired_token_before_usage_fetch` and
/// `test_refresh_failure_returns_none_gracefully`: an expired token sends nothing at all.
#[test]
fn expired_token_sends_no_request() {
    let sandbox = Sandbox::new();
    let credentials = account_credentials(&sandbox, Some(sandbox.env().now() * 1000. - 1000.));
    let outcome = try_fetch_usage_for_account(&sandbox.env(), "1", &credentials);
    assert_eq!(outcome.usage, None);
    assert_eq!(outcome.error.as_deref(), Some(TOKEN_EXPIRED_ERROR));
    assert_eq!(outcome.error.as_deref(), Some("token-expired"));
    assert!(sandbox.http.requests().is_empty());
}

/// cswap's `test_retries_401_with_token_refresh` and
/// `test_active_account_401_does_not_retry_with_refresh`: a 401 is its kind, with no refresh.
#[test]
fn a_401_is_returned_without_a_refresh() {
    let sandbox = Sandbox::new();
    sandbox.http.queue(USAGE_URL, Ok(answer(401, &[], "")));
    let outcome =
        try_fetch_usage_for_account(&sandbox.env(), "2", &account_credentials(&sandbox, None));
    assert_eq!(outcome.error.as_deref(), Some("http-401"));
    assert!(sandbox.http.requests_to(OAUTH_TOKEN_URL).is_empty());
    let requests = sandbox.http.requests_to(USAGE_URL);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        header(&requests[0], "Authorization"),
        Some("Bearer old-access")
    );
}

#[test]
fn valid_token_fetches_usage_without_refresh() {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue_json(USAGE_URL, 200, &usage_response(10., 20.));
    let outcome =
        try_fetch_usage_for_account(&sandbox.env(), "1", &account_credentials(&sandbox, None));
    assert_eq!(
        num(outcome.usage.as_ref().unwrap(), &["five_hour", "pct"]),
        Some(10.)
    );
    assert!(sandbox.http.requests_to(OAUTH_TOKEN_URL).is_empty());
    let [request] = sandbox.http.requests().try_into().unwrap();
    assert_eq!(header(&request, "Authorization"), Some("Bearer old-access"));
    assert_eq!(header(&request, "anthropic-beta"), Some(OAUTH_BETA_HEADER));
    assert_eq!(header(&request, "User-Agent"), Some(USER_AGENT));
    assert_eq!(request.timeout, std::time::Duration::from_secs(5));
}

/// cswap's `test_active_account_skips_refresh_even_when_expired`: no refresh POST, and here no
/// usage request either.
#[test]
fn active_account_skips_refresh_even_when_expired() {
    let sandbox = Sandbox::new();
    let credentials = account_credentials(&sandbox, Some(sandbox.env().now() * 1000. - 1000.));
    let outcome = try_fetch_usage_for_account(&sandbox.env(), "1", &credentials);
    assert_eq!(outcome.usage, None);
    assert!(sandbox.http.requests().is_empty());
}

// TestClassifyUsageError

fn http_error(code: u16, headers: &[(&str, &str)], body: Option<&str>) -> FetchError {
    FetchError::Status(answer(code, headers, body.unwrap_or_default()))
}

#[test]
fn http_codes() {
    assert_eq!(
        classify_usage_error(&http_error(429, &[], None)).0,
        "http-429"
    );
    assert_eq!(
        classify_usage_error(&http_error(500, &[], None)).0,
        "http-500"
    );
    assert_eq!(
        classify_usage_error(&http_error(401, &[], None)).0,
        "http-401"
    );
}

#[test]
fn _403_org_oauth_policy_names_itself() {
    let org = r#"{"error": {"details": {"error_code": "oauth_not_allowed_for_organization"}}}"#;
    for (code, body, kind) in [
        (403, Some(org), ORG_POLICY_ERROR),
        (401, Some(org), "http-401"),
        (403, None, "http-403"),
        (403, Some("<html>forbidden</html>"), "http-403"),
        (403, Some(r#"{"error": "x"}"#), "http-403"),
    ] {
        let error = http_error(code, &[("Retry-After", "7")], body);
        assert_eq!(
            classify_usage_error(&error),
            (kind.to_owned(), Some(7.)),
            "{code} {body:?}"
        );
    }
}

#[test]
fn retry_after_seconds() {
    let (kind, retry) = classify_usage_error(&http_error(429, &[("Retry-After", "30")], None));
    assert_eq!(kind, "http-429");
    assert_eq!(retry, Some(30.));
}

#[test]
fn retry_after_date_form_ignored() {
    let error = http_error(
        429,
        &[("Retry-After", "Fri, 04 Jul 2026 12:00:00 GMT")],
        None,
    );
    assert_eq!(classify_usage_error(&error).1, None);
}

#[test]
fn retry_after_negative_clamped() {
    let error = http_error(429, &[("Retry-After", "-5")], None);
    assert_eq!(classify_usage_error(&error).1, Some(0.));
}

/// The spec's rule: `Retry-After` is a bounded integer, where cswap's `float()` took `inf`.
#[test]
fn retry_after_is_a_bounded_integer() {
    assert_eq!(parse_retry_after(" 30 "), Some(30.));
    assert_eq!(parse_retry_after("+30"), Some(30.));
    assert_eq!(parse_retry_after("0"), Some(0.));
    assert_eq!(parse_retry_after("3600"), Some(3600.));
    assert_eq!(parse_retry_after("86400"), Some(86_400.));
    assert_eq!(parse_retry_after("86401"), Some(86_400.));
    assert_eq!(parse_retry_after("99999999999999999999999"), Some(86_400.));
    assert_eq!(RETRY_AFTER_BOUND_S, 86_400);
    for unusable in ["inf", "Infinity", "1e400", "nan", "1.5", "", "-", "soon"] {
        assert_eq!(parse_retry_after(unusable), None, "{unusable}");
    }
}

#[test]
fn no_headers() {
    assert_eq!(
        classify_usage_error(&http_error(429, &[], None)),
        ("http-429".to_owned(), None)
    );
}

#[test]
fn timeout() {
    let error = FetchError::Transport(HttpError::Timeout);
    assert_eq!(classify_usage_error(&error).0, "timeout");
}

#[test]
fn network() {
    let error = FetchError::Transport(HttpError::Network("Connection refused".into()));
    assert_eq!(classify_usage_error(&error).0, "network");
}

/// A refused certificate is `tls-cert`, a transport failure of any other kind `network`. Which
/// curl exits are a refused certificate is `http::failure`'s (`HttpError::Tls`).
#[test]
fn refused_certificate_is_told_apart() {
    let refused = FetchError::Transport(HttpError::Tls("certificate verify failed".into()));
    assert_eq!(
        classify_usage_error(&refused),
        ("tls-cert".to_owned(), None)
    );
    let other = FetchError::Transport(HttpError::Network("EOF occurred".into()));
    assert_eq!(classify_usage_error(&other), ("network".to_owned(), None));
}

#[test]
fn classify_bad_response() {
    let error = FetchError::BadResponse("expected value".into());
    assert_eq!(classify_usage_error(&error).0, "bad-response");
}

#[test]
fn fallback_type_name() {
    assert_eq!(
        classify_usage_error(&FetchError::Other("ValueError")).0,
        "ValueError"
    );
}

// TestTryFetchUsageOutcome

#[test]
fn success_outcome() {
    let sandbox = Sandbox::new();
    sandbox.http.queue_json(
        USAGE_URL,
        200,
        &json!({"five_hour": {"utilization": 12.0, "resets_at": null}}),
    );
    let outcome =
        try_fetch_usage_for_account(&sandbox.env(), "1", &account_credentials(&sandbox, None));
    assert_eq!(outcome.error, None);
    assert_eq!(
        num(outcome.usage.as_ref().unwrap(), &["five_hour", "pct"]),
        Some(12.)
    );
}

#[test]
fn _429_outcome_carries_retry_after() {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue(USAGE_URL, Ok(answer(429, &[("Retry-After", "42")], "")));
    let outcome =
        try_fetch_usage_for_account(&sandbox.env(), "1", &account_credentials(&sandbox, None));
    assert_eq!(outcome.usage, None);
    assert_eq!(outcome.error.as_deref(), Some("http-429"));
    assert_eq!(outcome.retry_after_s, Some(42.));
    // The line people paste into public issues: the account number and the server's
    // Retry-After, never the email; any 429 is the usage endpoint's own budget.
    let line = usage_failure_line("for account 1", "http-429", Some(42.));
    assert!(line.contains("account 1"), "{line}");
    assert!(line.contains("retry-after 42s"), "{line}");
    assert!(!line.contains("a@b.c"));
    assert!(line.contains("usage-endpoint budget"), "{line}");
}

#[test]
fn edge_429_warning_names_the_budget() {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue(USAGE_URL, Ok(answer(429, &[("Retry-After", "0")], "")));
    let outcome =
        try_fetch_usage_for_account(&sandbox.env(), "1", &account_credentials(&sandbox, None));
    assert_eq!(outcome.retry_after_s, Some(0.));
    let line = usage_failure_line("for account 1", "http-429", outcome.retry_after_s);
    assert!(line.contains("retry-after 0s"), "{line}");
    assert!(line.contains("usage-endpoint budget"), "{line}");
}

#[test]
fn timeout_outcome() {
    let sandbox = Sandbox::new();
    sandbox.http.queue(USAGE_URL, Err(HttpError::Timeout));
    let outcome =
        try_fetch_usage_for_account(&sandbox.env(), "1", &account_credentials(&sandbox, None));
    assert_eq!(outcome.error.as_deref(), Some("timeout"));
}

#[test]
fn no_access_token_outcome() {
    let sandbox = Sandbox::new();
    let creds = json!({"claudeAiOauth": {}}).to_string();
    let outcome = try_fetch_usage_for_account(&sandbox.env(), "1", &creds);
    assert_eq!(outcome.error.as_deref(), Some("no-access-token"));
}

// TestSetupTokenHeaderProbe, the cases that hold without the probe

#[test]
fn disabled_sends_no_request() {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue(USAGE_URL, Ok(answer(429, &[("Retry-After", "318")], "")));
    let creds = json!({"claudeAiOauth": {
        "accessToken": "sk-ant-oat01-secret-token", "scopes": ["user:inference"],
    }})
    .to_string();
    let outcome = try_fetch_usage_for_account(&sandbox.env(), "3", &creds);
    assert_eq!(outcome.usage, None);
    assert_eq!(outcome.error.as_deref(), Some("setup-token"));
    assert!(sandbox.http.requests().is_empty());
}

#[test]
fn login_credentials_never_probe() {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue(USAGE_URL, Ok(answer(429, &[("Retry-After", "318")], "")));
    let future_ms = (sandbox.env().now() + 3600.) * 1000.;
    let creds = json!({"claudeAiOauth": {
        "accessToken": "a", "refreshToken": "r", "expiresAt": future_ms as i64,
        "scopes": ["user:inference", "user:profile"],
    }})
    .to_string();
    let outcome = try_fetch_usage_for_account(&sandbox.env(), "3", &creds);
    assert_eq!(outcome.error.as_deref(), Some("http-429"));
    let urls: Vec<String> = sandbox
        .http
        .requests()
        .into_iter()
        .map(|request| request.url)
        .collect();
    assert_eq!(urls, [USAGE_URL]);
}

//! From claude-swap's `tests/test_oauth.py` (0fe22a8):
//! `TestRefreshOAuthCredentials`, `TestTryRefreshOAuthCredentials`, `TestTokenAccountParsing`,
//! `TestFetchOauthProfile`, `TestInvalidGrantTaxonomy`, `TestNoRefreshTokenStructuralGuard`.

use std::time::Duration;

use serde_json::{Value, json};

use super::{answer, header};
use crate::http::HttpError;
use crate::oauth::*;
use crate::testing::{Sandbox, json_response};

fn make_credentials() -> String {
    json!({"claudeAiOauth": {
        "accessToken": "old-access",
        "refreshToken": "old-refresh",
        "expiresAt": 0,
        "scopes": ["user:profile", "user:inference", "user:sessions:claude_code"],
    }})
    .to_string()
}

fn grant() -> Value {
    json!({"access_token": "new-access", "refresh_token": "new-refresh", "expires_in": 3600})
}

fn refresh(sandbox: &Sandbox, credentials: &str) -> RefreshOutcome {
    try_refresh_oauth_credentials(&sandbox.env(), credentials, Duration::from_secs(10))
}

fn refused(code: u16, body: &str) -> RefreshOutcome {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue(OAUTH_TOKEN_URL, Ok(answer(code, &[], body)));
    refresh(&sandbox, &make_credentials())
}

// TestRefreshOAuthCredentials

#[test]
fn refresh_sends_correct_body() {
    let sandbox = Sandbox::new();
    sandbox.http.queue_json(OAUTH_TOKEN_URL, 200, &grant());
    let refreshed = refresh_oauth_credentials(&sandbox.env(), &make_credentials());
    assert!(refreshed.is_some());
    let [request] = sandbox
        .http
        .requests_to(OAUTH_TOKEN_URL)
        .try_into()
        .unwrap();
    let body: Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["grant_type"], "refresh_token");
    assert_eq!(body["refresh_token"], "old-refresh");
    assert_eq!(body["client_id"], OAUTH_CLIENT_ID);
    assert!(body.get("scope").is_none());
    assert_eq!(request.method, "POST");
    assert_eq!(request.timeout, Duration::from_secs(10));
    // The token endpoint answers 403 without a User-Agent.
    assert_eq!(header(&request, "User-Agent"), Some(USER_AGENT));
    assert_eq!(header(&request, "Content-Type"), Some("application/json"));
}

// TestTryRefreshOAuthCredentials

#[test]
fn success_rotates_and_has_no_error() {
    let sandbox = Sandbox::new();
    sandbox.http.queue_json(OAUTH_TOKEN_URL, 200, &grant());
    let outcome = refresh(&sandbox, &make_credentials());
    assert_eq!(outcome.error, None);
    let rotated: Value = serde_json::from_str(&outcome.credentials.unwrap()).unwrap();
    assert_eq!(rotated["claudeAiOauth"]["accessToken"], "new-access");
    assert_eq!(rotated["claudeAiOauth"]["refreshToken"], "new-refresh");
    let now_ms = (sandbox.env().now() * 1000.) as i64;
    assert_eq!(
        rotated["claudeAiOauth"]["expiresAt"].as_i64(),
        Some(now_ms + 3_600_000)
    );
}

#[test]
fn invalid_grant_body_on_400_is_permanent() {
    let outcome = refused(400, r#"{"error": "invalid_grant"}"#);
    assert_eq!(outcome.credentials, None);
    assert_eq!(outcome.error.as_deref(), Some("invalid_grant"));
}

#[test]
fn _400_without_marker_is_transient() {
    let outcome = refused(400, r#"{"error": "temporarily_unavailable"}"#);
    assert_eq!(outcome.error.as_deref(), Some("transient"));
}

#[test]
fn _5xx_is_transient_even_with_marker() {
    let outcome = refused(500, r#"{"error": "invalid_grant"}"#);
    assert_eq!(outcome.error.as_deref(), Some("transient"));
}

#[test]
fn network_error_is_transient() {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue(OAUTH_TOKEN_URL, Err(HttpError::Network("dns".into())));
    let outcome = refresh(&sandbox, &make_credentials());
    assert_eq!(outcome.error.as_deref(), Some("transient"));
}

#[test]
fn missing_refresh_token_is_permanent() {
    let creds = json!({"claudeAiOauth": {"accessToken": "a", "expiresAt": 0}}).to_string();
    let outcome = refresh(&Sandbox::new(), &creds);
    assert_eq!(outcome.error.as_deref(), Some("no_refresh_token"));
}

#[test]
fn invalid_json_is_transient() {
    // An unparseable blob is likelier a torn read than a credential shape: no permanent verdict.
    let outcome = refresh(&Sandbox::new(), "not json");
    assert_eq!(outcome.error.as_deref(), Some("transient"));
}

#[test]
fn wrapper_returns_none_on_failure() {
    let sandbox = Sandbox::new();
    sandbox.http.queue(
        OAUTH_TOKEN_URL,
        Ok(json_response(400, &json!({"error": "invalid_grant"}))),
    );
    assert_eq!(
        refresh_oauth_credentials(&sandbox.env(), &make_credentials()),
        None
    );
}

// TestTokenAccountParsing

fn refresh_with_response(payload: &Value) -> RefreshOutcome {
    let sandbox = Sandbox::new();
    sandbox.http.queue_json(OAUTH_TOKEN_URL, 200, payload);
    refresh(&sandbox, &make_credentials())
}

fn with_identity(extra: &Value) -> Value {
    let mut payload = grant();
    for (key, value) in extra.as_object().unwrap() {
        payload[key] = value.clone();
    }
    payload
}

fn identity(uuid: &str, email: Option<&str>, org: Option<&str>) -> AccountIdentity {
    AccountIdentity {
        uuid: uuid.to_owned(),
        email: email.map(str::to_owned),
        organization_uuid: org.map(str::to_owned),
    }
}

#[test]
fn token_account_surfaced_when_present() {
    let outcome = refresh_with_response(&with_identity(&json!({
        "account": {"uuid": "acc-uuid", "email_address": "a@b.c"},
        "organization": {"uuid": "org-uuid"},
    })));
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.token_account,
        Some(identity("acc-uuid", Some("a@b.c"), Some("org-uuid")))
    );
    // The identity's stored shape: cswap's keys.
    assert_eq!(
        serde_json::to_value(outcome.token_account.unwrap()).unwrap(),
        json!({"uuid": "acc-uuid", "email": "a@b.c", "organizationUuid": "org-uuid"})
    );
}

#[test]
fn token_account_absent_is_none() {
    let outcome = refresh_with_response(&grant());
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.token_account, None);
}

#[test]
fn token_account_without_uuid_is_none() {
    let outcome = refresh_with_response(&with_identity(
        &json!({"account": {"email_address": "a@b.c"}}),
    ));
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.token_account, None);
}

#[test]
fn token_account_non_string_uuid_is_none() {
    let outcome = refresh_with_response(&with_identity(
        &json!({"account": {"uuid": 12345, "email_address": "a@b.c"}}),
    ));
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.token_account, None);
}

#[test]
fn token_account_uuid_whitespace_normalized() {
    let outcome = refresh_with_response(&with_identity(
        &json!({"account": {"uuid": "  acc-uuid  ", "email_address": "a@b.c"}}),
    ));
    assert_eq!(outcome.token_account.unwrap().uuid, "acc-uuid");
}

#[test]
fn token_account_non_string_optionals_normalized() {
    let outcome = refresh_with_response(&with_identity(&json!({
        "account": {"uuid": "acc-uuid", "email_address": {"weird": 1}},
        "organization": {"uuid": 99},
    })));
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.token_account,
        Some(identity("acc-uuid", None, None))
    );
}

// TestFetchOauthProfile

fn profile(payload: &Value) -> Option<AccountIdentity> {
    let sandbox = Sandbox::new();
    sandbox.http.queue_json(PROFILE_URL, 200, payload);
    fetch_oauth_profile(&sandbox.env(), "sk-live")
}

#[test]
fn resolves_identity() {
    let sandbox = Sandbox::new();
    sandbox.http.queue_json(
        PROFILE_URL,
        200,
        &json!({"account": {"uuid": "acc-uuid", "email": "a@b.c"}, "organization": {"uuid": "org-uuid"}}),
    );
    let result = fetch_oauth_profile(&sandbox.env(), "sk-live");
    assert_eq!(
        result,
        Some(identity("acc-uuid", Some("a@b.c"), Some("org-uuid")))
    );
    let [request] = sandbox.http.requests().try_into().unwrap();
    assert!(request.url.ends_with("/api/oauth/profile"));
    assert_eq!(header(&request, "Authorization"), Some("Bearer sk-live"));
}

#[test]
fn uses_bounded_timeout() {
    let sandbox = Sandbox::new();
    sandbox.http.queue_json(
        PROFILE_URL,
        200,
        &json!({"account": {"uuid": "acc-uuid", "email": "a@b.c"}}),
    );
    fetch_oauth_profile(&sandbox.env(), "sk-live");
    assert_eq!(sandbox.http.requests()[0].timeout, Duration::from_secs(5));
}

#[test]
fn network_failure_is_unresolvable_not_error() {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue(PROFILE_URL, Err(HttpError::Network("down".into())));
    assert_eq!(fetch_oauth_profile(&sandbox.env(), "sk-live"), None);
}

#[test]
fn missing_account_object_is_unresolvable() {
    assert_eq!(profile(&json!({"unexpected": true})), None);
}

#[test]
fn missing_uuid_is_unresolvable() {
    assert_eq!(
        profile(&json!({"account": {"email": "a@b.c"}, "organization": {"uuid": "org-uuid"}})),
        None
    );
}

#[test]
fn non_string_uuid_is_unresolvable() {
    assert_eq!(
        profile(&json!({"account": {"uuid": 12345, "email": "a@b.c"}})),
        None
    );
}

#[test]
fn blank_uuid_is_unresolvable() {
    assert_eq!(
        profile(&json!({"account": {"uuid": "   ", "email": "a@b.c"}})),
        None
    );
}

#[test]
fn malformed_json_is_unresolvable() {
    let sandbox = Sandbox::new();
    sandbox.http.queue(
        PROFILE_URL,
        Ok(answer(200, &[], "<!doctype html><html>gateway error")),
    );
    assert_eq!(fetch_oauth_profile(&sandbox.env(), "sk-live"), None);
}

#[test]
fn uuid_whitespace_normalized_at_boundary() {
    let result = profile(&json!({"account": {"uuid": "  acc-uuid  ", "email": "a@b.c"}}));
    assert_eq!(result.unwrap().uuid, "acc-uuid");
}

#[test]
fn valid_uuid_with_missing_email_still_resolves() {
    assert_eq!(
        profile(&json!({"account": {"uuid": "acc-uuid"}})),
        Some(identity("acc-uuid", None, None))
    );
}

#[test]
fn non_string_optional_fields_are_dropped_not_fatal() {
    assert_eq!(
        profile(&json!({
            "account": {"uuid": "acc-uuid", "email": {"weird": true}},
            "organization": {"uuid": 99},
        })),
        Some(identity("acc-uuid", None, None))
    );
}

#[test]
fn _401_is_unresolvable_with_log_file_warning() {
    let sandbox = Sandbox::new();
    sandbox.http.queue(PROFILE_URL, Ok(answer(401, &[], "")));
    assert_eq!(fetch_oauth_profile(&sandbox.env(), "sk-live"), None);
    // The warning it logs names the status and the fail-open path, never the token.
    assert!(PROFILE_401_WARNING.contains("401") && PROFILE_401_WARNING.contains("pre-fix"));
}

// TestInvalidGrantTaxonomy

fn refresh_with_body(code: u16, body: &str) -> RefreshOutcome {
    let sandbox = Sandbox::new();
    sandbox
        .http
        .queue(OAUTH_TOKEN_URL, Ok(answer(code, &[], body)));
    let creds = json!({"claudeAiOauth": {"refreshToken": "rt-x", "accessToken": "a"}}).to_string();
    refresh(&sandbox, &creds)
}

#[test]
fn rfc_invalid_grant_is_permanent() {
    let out = refresh_with_body(400, r#"{"error": "invalid_grant"}"#);
    assert_eq!(out.error.as_deref(), Some("invalid_grant"));
}

#[test]
fn substring_in_other_envelope_is_transient() {
    let out = refresh_with_body(
        400,
        r#"{"error": "server_error", "detail": "log mentions invalid_grant"}"#,
    );
    assert_eq!(out.error.as_deref(), Some("transient"));
}

#[test]
fn invalid_client_is_systemic_not_dead_token() {
    let out = refresh_with_body(401, r#"{"error": "invalid_client"}"#);
    assert_eq!(out.error.as_deref(), Some("invalid_client"));
}

#[test]
fn unparseable_body_is_transient() {
    let out = refresh_with_body(400, "<html>oops</html>");
    assert_eq!(out.error.as_deref(), Some("transient"));
}

#[test]
fn error_description_variant_still_permanent() {
    let out = refresh_with_body(
        400,
        r#"{"error": "invalid_grant", "error_description": "revoked"}"#,
    );
    assert_eq!(out.error.as_deref(), Some("invalid_grant"));
}

// TestNoRefreshTokenStructuralGuard

#[test]
fn complete_dict_without_rt_is_permanent() {
    let creds = json!({"claudeAiOauth": {"accessToken": "a"}}).to_string();
    assert_eq!(
        refresh(&Sandbox::new(), &creds).error.as_deref(),
        Some("no_refresh_token")
    );
}

#[test]
fn unparseable_blob_is_transient() {
    assert_eq!(
        refresh(&Sandbox::new(), r#"{"claudeAiOa"#).error.as_deref(),
        Some("transient")
    );
}

#[test]
fn non_dict_payload_is_transient() {
    assert_eq!(
        refresh(&Sandbox::new(), r#""just-a-string""#)
            .error
            .as_deref(),
        Some("transient")
    );
}

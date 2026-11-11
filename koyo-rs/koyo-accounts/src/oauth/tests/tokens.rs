//! From claude-swap's `tests/test_oauth.py` (0fe22a8):
//! `TestExtractAccessToken`, `TestBuildTokenStatus`, `TestIsSetupToken`,
//! `TestCredentialFingerprint`, `TestLoginExpiresAtIso`.

use serde_json::{Map, Value, json};

use super::utc;
use crate::oauth::*;

fn oauth(value: Value) -> Map<String, Value> {
    let Value::Object(map) = value else {
        panic!("not an object: {value}")
    };
    map
}

// TestExtractAccessToken

#[test]
fn valid_credentials() {
    let creds = json!({"claudeAiOauth": {"accessToken": "sk-test-token"}}).to_string();
    assert_eq!(
        extract_access_token(&creds).as_deref(),
        Some("sk-test-token")
    );
}

#[test]
fn missing_key() {
    let creds = json!({"claudeAiOauth": {}}).to_string();
    assert_eq!(extract_access_token(&creds), None);
}

#[test]
fn invalid_json() {
    assert_eq!(extract_access_token("not-json"), None);
}

#[test]
fn empty_string() {
    assert_eq!(extract_access_token(""), None);
}

// TestBuildTokenStatus

#[test]
fn builds_fresh_token_status() {
    let now = utc(2026, 4, 2, 18, 0, 0);
    let expires_at = utc(2026, 4, 2, 19, 30, 0) * 1000.;
    let credentials = json!({"claudeAiOauth": {
        "accessToken": "old-access", "refreshToken": "old-refresh", "expiresAt": expires_at as i64,
    }})
    .to_string();
    let status = build_token_status(&credentials, now).unwrap();
    assert!(
        status.contains("oauth: fresh, refresh token yes"),
        "{status}"
    );
    assert!(status.contains("in 1h 30m"), "{status}");
    assert!(!status.contains("old-access") && !status.contains("old-refresh"));
}

#[test]
fn builds_unknown_expiry_status() {
    let credentials = json!({"claudeAiOauth": {
        "accessToken": "old-access", "refreshToken": "old-refresh",
    }})
    .to_string();
    assert_eq!(
        build_token_status(&credentials, 0.).as_deref(),
        Some("oauth: unknown expiry, refresh token yes")
    );
}

// TestIsSetupToken

#[test]
fn add_token_shape() {
    assert!(is_setup_token(Some(&oauth(
        json!({"accessToken": "t", "scopes": ["user:inference"]})
    ))));
}

#[test]
fn login_with_refresh_token_is_not() {
    assert!(!is_setup_token(Some(&oauth(json!({
        "accessToken": "t", "refreshToken": "r", "scopes": ["user:inference"],
    })))));
}

#[test]
fn profile_scope_is_not() {
    assert!(!is_setup_token(Some(&oauth(json!({
        "accessToken": "t", "scopes": ["user:inference", "user:profile"],
    })))));
}

#[test]
fn missing_scopes_is_not() {
    // Unknown provenance: keep the endpoint's answer, don't spend quota.
    assert!(!is_setup_token(Some(&oauth(json!({"accessToken": "t"})))));
    assert!(!is_setup_token(None));
}

// TestCredentialFingerprint

#[test]
fn stable_across_access_token_rotation() {
    let a = json!({"claudeAiOauth": {"accessToken": "sk-old", "refreshToken": "rt-1"}});
    let b =
        json!({"claudeAiOauth": {"accessToken": "sk-new", "refreshToken": "rt-1", "expiresAt": 5}});
    assert_eq!(
        credential_fingerprint(&a.to_string()),
        credential_fingerprint(&b.to_string())
    );
}

#[test]
fn differs_across_refresh_token_rotation() {
    let a = json!({"claudeAiOauth": {"refreshToken": "rt-1"}}).to_string();
    let b = json!({"claudeAiOauth": {"refreshToken": "rt-2"}}).to_string();
    assert_ne!(credential_fingerprint(&a), credential_fingerprint(&b));
}

#[test]
fn full_content_fallback_for_api_keys_and_setup_tokens() {
    let api_key = "sk-ant-api03-xyz";
    let setup = json!({"claudeAiOauth": {"accessToken": "sk-ant-oat01-abc"}}).to_string();
    assert!(credential_fingerprint(api_key).is_some());
    assert!(credential_fingerprint(&setup).is_some());
    // Never None for real bytes: a None would make every "did it change?" read "changed".
    assert_ne!(
        credential_fingerprint(api_key),
        credential_fingerprint(&setup)
    );
}

#[test]
fn full_hash_never_collides_with_refresh_hash() {
    let with_rt = json!({"claudeAiOauth": {"refreshToken": "rt-1"}}).to_string();
    assert!(
        credential_fingerprint(&with_rt)
            .unwrap()
            .starts_with("sha256:")
    );
    assert!(
        credential_fingerprint("raw-token")
            .unwrap()
            .starts_with("sha256-full:")
    );
}

#[test]
fn empty_input_is_none() {
    assert_eq!(credential_fingerprint(""), None);
}

#[test]
fn access_token_fingerprint_follows_the_access_token() {
    let a = json!({"claudeAiOauth": {"accessToken": "sk-old", "refreshToken": "rt-1"}}).to_string();
    let b = json!({"claudeAiOauth": {"accessToken": "sk-new", "refreshToken": "rt-1"}}).to_string();
    assert!(
        access_token_fingerprint(&a)
            .unwrap()
            .starts_with("sha256-at:")
    );
    assert_ne!(access_token_fingerprint(&a), access_token_fingerprint(&b));
    assert_eq!(access_token_fingerprint("{}"), None);
}

// TestLoginExpiresAtIso

#[test]
fn refresh_token_expiry_is_reported_as_iso_utc() {
    let creds = json!({"claudeAiOauth": {
        "accessToken": "sk-x", "refreshTokenExpiresAt": 1_791_421_596_865_i64,
    }})
    .to_string();
    assert_eq!(
        login_expires_at_iso(&creds).as_deref(),
        Some("2026-10-08T01:06:36Z")
    );
    assert_eq!(login_expires_at_epoch(&creds), Some(1_791_421_596.865));
}

#[test]
fn anything_but_a_positive_epoch_is_unknown() {
    // json.dumps writes NaN and Infinity as these literals, which serde refuses, as the
    // credential then reads as unparseable.
    for creds in [
        String::new(),
        "not json".to_owned(),
        json!({"claudeAiOauth": {"accessToken": "sk-x"}}).to_string(),
        json!({"claudeAiOauth": {"refreshTokenExpiresAt": "soon"}}).to_string(),
        json!({"claudeAiOauth": {"refreshTokenExpiresAt": true}}).to_string(),
        json!({"claudeAiOauth": {"refreshTokenExpiresAt": 0}}).to_string(),
        r#"{"claudeAiOauth": {"refreshTokenExpiresAt": NaN}}"#.to_owned(),
        r#"{"claudeAiOauth": {"refreshTokenExpiresAt": Infinity}}"#.to_owned(),
        json!({"other": {}}).to_string(),
    ] {
        assert_eq!(login_expires_at_iso(&creds), None, "{creds}");
    }
}

#[test]
fn login_expiry_is_said_within_a_week() {
    let now = 1_000_000.;
    assert_eq!(login_time_left(None, false, now), None);
    assert_eq!(login_time_left(Some(now + 8. * 86_400.), false, now), None);
    assert_eq!(login_time_left(Some(now + 86_400.), true, now), None);
    assert_eq!(
        login_time_left(Some(now + 86_400.), false, now),
        Some(86_400.)
    );
    assert_eq!(login_time_left(Some(now - 5.), false, now), Some(0.));
    let soon = Some(now + 5. * 86_400. + 12. * 3600.);
    assert_eq!(
        format_login_expiry(soon, false, now, false).as_deref(),
        Some("re-login in 5d 12h")
    );
    assert_eq!(
        format_login_expiry(soon, false, now, true).as_deref(),
        Some("re-login in 5d")
    );
    assert_eq!(
        format_login_expiry(Some(now), false, now, false).as_deref(),
        Some("re-login needed")
    );
}

#[test]
fn is_oauth_token_expired_keeps_a_five_minute_buffer() {
    let now = 1_000_000.;
    let ms = |seconds: f64| Some(json!(seconds * 1000.));
    assert!(!is_oauth_token_expired(ms(now + 301.).as_ref(), now));
    assert!(is_oauth_token_expired(ms(now + 300.).as_ref(), now));
    assert!(is_oauth_token_expired(ms(now - 1.).as_ref(), now));
    assert!(!is_oauth_token_expired(Some(&json!("soon")), now));
    assert!(!is_oauth_token_expired(None, now));
}

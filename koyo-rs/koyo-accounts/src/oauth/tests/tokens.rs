//! From claude-swap's `tests/test_oauth.py` (0fe22a8): `TestExtractAccessToken`,
//! `TestIsSetupToken`, `TestCredentialFingerprint`. `TestBuildTokenStatus` and
//! `TestLoginExpiresAtIso` are left out with the code they test, which nothing called.

use serde_json::{Map, Value, json};

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

//! `test_api_key_accounts.py`'s `TestKindDetection` (its key cases) and `test_switcher.py`'s
//! `TestSharedOAuthCredentialPreservation` cases that compose a credential directly. cswap
//! drives those through the switcher's `_prepare_credentials_for_activation`, which is
//! [`prepare`] below: the shared fields of the live credential, merged into the target.

use serde_json::{Value, json};

use crate::credentials::{
    looks_like_api_key, merge_shared_credential_fields, shared_credential_fields,
};

const API_KEY: &str = "sk-ant-api03-a1b2c3d4e5a1b2c3d4e5a1b2c3d4e5a1b2c3d4e5";

fn key(prefix: &str, unit: &str, times: usize) -> String {
    format!("{prefix}{}", unit.repeat(times))
}

/// The switcher's `_prepare_credentials_for_activation`.
fn prepare(target: &str, live: Option<&str>) -> String {
    match shared_credential_fields(live) {
        None => target.to_owned(),
        Some(shared) => merge_shared_credential_fields(target, &shared),
    }
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn api_key_detected() {
    for value in [
        key("sk-ant-api03-", "a1b2c3d4e5", 4),
        key("sk-ant-usr-", "u1v2w3x4y5", 9),
        // A family that doesn't exist yet is still a key: a denylist of OAuth prefixes.
        key("sk-ant-new99-", "k", 40),
        key("sk-ant-admin01-", "d", 40),
    ] {
        assert!(looks_like_api_key(Some(&value)), "{value}");
        assert!(looks_like_api_key(Some(&format!("  {value}\n"))), "{value}");
    }
}

#[test]
fn non_api_key() {
    let setup = key("sk-ant-oat01-", "o9p8q7r6s5", 4);
    let oauth = json!({"claudeAiOauth": {"accessToken": "sk-ant-oat01-x"}}).to_string();
    for value in [
        Some(String::new()),
        None,
        Some(setup.clone()),
        Some(format!("  {setup}\n")),
        Some(key("sk-ant-ort01-", "r4s3t2u1v0", 4)),
        Some("sk-ant-oat".to_owned()),
        Some("not-a-key".to_owned()),
        Some(oauth),
        Some(r#"{"x": "sk-ant-api03-inside-json"}"#.to_owned()),
        Some(r#"{"x": "sk-ant-usr-inside-json"}"#.to_owned()),
    ] {
        assert!(!looks_like_api_key(value.as_deref()), "{value:?}");
    }
}

#[test]
fn live_shared_keys_win_over_stale_target_copies() {
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "mcpOAuth": {"server": {"refreshToken": "stale"}},
        "mcpOAuthClientConfig": {"server": {"clientId": "stale"}},
    })
    .to_string();
    let live = json!({
        "claudeAiOauth": {"accessToken": "live"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
        "mcpOAuthClientConfig": {"server": {"clientId": "current"}},
    })
    .to_string();
    assert_eq!(
        parsed(&prepare(&target, Some(&live))),
        json!({
            "claudeAiOauth": {"accessToken": "target"},
            "mcpOAuth": {"server": {"refreshToken": "current"}},
            "mcpOAuthClientConfig": {"server": {"clientId": "current"}},
        })
    );
}

#[test]
fn account_bound_and_unknown_siblings_stay_target_owned() {
    // trustedDeviceToken is enrolled per account, and an unknown field could be too.
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "trustedDeviceToken": "device-token-b",
        "someFutureField": {"value": "target-owned"},
    })
    .to_string();
    let live = json!({
        "claudeAiOauth": {"accessToken": "live"},
        "trustedDeviceToken": "device-token-a",
        "someFutureField": {"value": "live"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
    })
    .to_string();
    assert_eq!(
        parsed(&prepare(&target, Some(&live))),
        json!({
            "claudeAiOauth": {"accessToken": "target"},
            "trustedDeviceToken": "device-token-b",
            "someFutureField": {"value": "target-owned"},
            "mcpOAuth": {"server": {"refreshToken": "current"}},
        })
    );
}

#[test]
fn shared_key_absent_from_live_is_not_resurrected() {
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "mcpOAuth": {"server": {"refreshToken": "stale"}},
    })
    .to_string();
    let live = json!({"claudeAiOauth": {"accessToken": "live"}}).to_string();
    assert_eq!(
        parsed(&prepare(&target, Some(&live))),
        json!({"claudeAiOauth": {"accessToken": "target"}})
    );
}

#[test]
fn api_key_live_state_activates_target_verbatim() {
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "mcpOAuth": {"server": {"refreshToken": "stale"}},
    })
    .to_string();
    assert_eq!(prepare(&target, Some(API_KEY)), target);
}

#[test]
fn absent_live_state_activates_target_verbatim() {
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "mcpOAuth": {"server": {"refreshToken": "old"}},
    })
    .to_string();
    assert_eq!(prepare(&target, Some("")), target);
    assert_eq!(prepare(&target, None), target);
}

#[test]
fn api_key_target_is_never_composed() {
    let live = json!({
        "claudeAiOauth": {"accessToken": "live"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
    })
    .to_string();
    assert_eq!(prepare(API_KEY, Some(&live)), API_KEY);
}

#[test]
fn opaque_target_credential_activates_verbatim() {
    let target = json!({"accessToken": "legacy", "refreshToken": "legacy-rt"}).to_string();
    let live = json!({
        "claudeAiOauth": {"accessToken": "live"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
    })
    .to_string();
    assert_eq!(prepare(&target, Some(&live)), target);
}

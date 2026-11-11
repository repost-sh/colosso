//! cswap's `tests/test_switcher.py` (0fe22a8) `TestDirectActivationPreservation` (but
//! `test_force_proceeds_with_warning_when_stash_fails`: `--force` isn't ported) and
//! `TestSharedOAuthCredentialPreservation`. The live credential is Claude Code's
//! `.credentials.json` here, as cswap's Linux fixture has it: with no Keychain item a read
//! falls back to it, and a write bumps it with the bytes written.

use std::os::unix::fs::PermissionsExt as _;

use serde_json::{Value, json};

use super::switch::two_accounts;
use super::{Harness, sample_sequence_data};
use crate::AccountsError;
use crate::switcher::Switcher;
use crate::switcher::store::WRITE_FAULT;
use crate::testing::creds;

/// cswap's `_setup`: slot 1 (one@) with its backups, an unmanaged live login in the file, and
/// a config naming `live_identity_email` (none at all for `None`).
fn setup(live_identity_email: Option<&str>) -> (Harness, String) {
    let h = Harness::new();
    if let Some(email) = live_identity_email {
        h.write_config(&json!({"oauthAccount": {
            "emailAddress": email, "accountUuid": "", "organizationUuid": null,
            "organizationName": null,
        }}));
    }
    h.write_sequence(&json!({
        "activeAccountNumber": null, "lastUpdated": "2024-01-01T00:00:00Z", "sequence": [1],
        "accounts": {"1": {
            "email": "one@example.com", "uuid": "uuid-one", "organizationUuid": "",
            "organizationName": "", "added": "2024-01-01T00:00:00Z",
        }},
    }));
    h.switcher
        .store
        .write_account_credentials("1", "one@example.com", &creds("sk-one", "rt-one", None))
        .unwrap();
    h.set_config_backup(
        "1",
        "one@example.com",
        &json!({"oauthAccount": {"emailAddress": "one@example.com", "accountUuid": "uuid-one"}}),
    );
    let unmanaged = creds("sk-unmanaged", "rt-unmanaged", None);
    std::fs::write(h.credentials_file(), &unmanaged).unwrap();
    (h, unmanaged)
}

fn live_file(h: &Harness) -> Value {
    serde_json::from_str(&std::fs::read_to_string(h.credentials_file()).unwrap()).unwrap()
}

#[test]
fn unmanaged_live_login_stashed_before_activation() {
    let (h, unmanaged) = setup(Some("untracked@example.com"));
    h.switcher.perform_switch("1", None).unwrap();
    let id = h.only_stash_id();
    assert_eq!(h.safety_copy(&id), unmanaged);
    assert_eq!(h.stash()[&id]["reason"], "displaced-live-login");
}

#[test]
fn stash_failure_aborts_direct_activation() {
    let (h, unmanaged) = setup(Some("untracked@example.com"));
    let stash_dir = h.sandbox.store().join("credentials");
    std::fs::set_permissions(&stash_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let result = h.switcher.perform_switch("1", None);
    std::fs::set_permissions(&stash_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let Err(AccountsError::Switch(why)) = result else {
        panic!("no safety copy, no activation: {result:?}");
    };
    assert!(why.contains("preserve the live credential"), "{why}");
    assert_eq!(
        std::fs::read_to_string(h.credentials_file()).unwrap(),
        unmanaged
    );
}

#[test]
fn orphaned_live_login_without_config_identity_is_stashed() {
    // The config is gone but a live login remains: it still gets its safety copy.
    let (h, orphaned) = setup(None);
    h.switcher.perform_switch("1", None).unwrap();
    let id = h.only_stash_id();
    assert_eq!(h.safety_copy(&id), orphaned);
    assert_eq!(h.stash()[&id]["reason"], "displaced-live-login");
    assert_eq!(live_file(&h)["claudeAiOauth"]["accessToken"], "sk-one");
}

#[test]
fn unreadable_live_credentials_without_config_identity_abort() {
    let (h, orphaned) = setup(None);
    let file = h.credentials_file();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = h.switcher.perform_switch("1", None);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let Err(AccountsError::CredentialRead(why)) = result else {
        panic!("an unreadable live credential must abort: {result:?}");
    };
    assert!(why.contains("snapshot"), "{why}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), orphaned);
}

#[test]
fn mid_failure_restores_identityless_config() {
    // A settings-bearing config without `oauthAccount` is restored with the login.
    let (h, orphaned) = setup(None);
    let original_config = json!({"projects": {"/home/x": {"history": []}}}).to_string();
    std::fs::write(h.config_path(), &original_config).unwrap();
    let sequence = h.switcher.sequence_file.clone();
    WRITE_FAULT.set(Some(Box::new(move |path, _| {
        (path == sequence).then(|| std::io::Error::other("disk full"))
    })));
    let result = h.switcher.perform_switch("1", None);
    WRITE_FAULT.set(None);
    assert!(result.unwrap_err().to_string().contains("disk full"));
    assert_eq!(
        std::fs::read_to_string(h.config_path()).unwrap(),
        original_config
    );
    assert_eq!(
        std::fs::read_to_string(h.credentials_file()).unwrap(),
        orphaned
    );
}

fn compose(target: &Value, live: Option<&str>) -> Value {
    serde_json::from_str(&Switcher::prepare_credentials_for_activation(
        &target.to_string(),
        live,
    ))
    .unwrap()
}

#[test]
fn live_shared_keys_win_over_stale_target_copies() {
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "mcpOAuth": {"server": {"refreshToken": "stale"}},
        "mcpOAuthClientConfig": {"server": {"clientId": "stale"}},
    });
    let live = json!({
        "claudeAiOauth": {"accessToken": "live"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
        "mcpOAuthClientConfig": {"server": {"clientId": "current"}},
    });
    assert_eq!(
        compose(&target, Some(&live.to_string())),
        json!({
            "claudeAiOauth": {"accessToken": "target"},
            "mcpOAuth": {"server": {"refreshToken": "current"}},
            "mcpOAuthClientConfig": {"server": {"clientId": "current"}},
        })
    );
}

#[test]
fn account_bound_and_unknown_siblings_stay_target_owned() {
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "trustedDeviceToken": "device-token-b",
        "someFutureField": {"value": "target-owned"},
    });
    let live = json!({
        "claudeAiOauth": {"accessToken": "live"},
        "trustedDeviceToken": "device-token-a",
        "someFutureField": {"value": "live"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
    });
    assert_eq!(
        compose(&target, Some(&live.to_string())),
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
    });
    let live = json!({"claudeAiOauth": {"accessToken": "live"}});
    assert_eq!(
        compose(&target, Some(&live.to_string())),
        json!({"claudeAiOauth": {"accessToken": "target"}})
    );
}

#[test]
fn direct_activation_without_config_identity_composes_live_state() {
    // An orphaned live credential still carries the machine's MCP state.
    let h = Harness::new();
    h.write_sequence(&json!({
        "activeAccountNumber": null, "lastUpdated": "2024-01-01T00:00:00Z", "sequence": [1],
        "accounts": {"1": {
            "email": "one@example.com", "uuid": "uuid-one", "organizationUuid": "",
            "organizationName": "", "added": "2024-01-01T00:00:00Z",
        }},
    }));
    let target = json!({
        "claudeAiOauth": {"accessToken": "sk-one", "refreshToken": "rt-one"},
        "mcpOAuth": {"server": {"refreshToken": "stale"}},
    });
    h.set_backup("1", "one@example.com", &target.to_string());
    h.set_config_backup(
        "1",
        "one@example.com",
        &json!({"oauthAccount": {"emailAddress": "one@example.com", "accountUuid": "uuid-one"}}),
    );
    std::fs::write(
        h.credentials_file(),
        json!({"mcpOAuth": {"server": {"refreshToken": "current"}}}).to_string(),
    )
    .unwrap();
    h.switcher.perform_switch("1", None).unwrap();
    assert_eq!(
        live_file(&h),
        json!({
            "claudeAiOauth": {"accessToken": "sk-one", "refreshToken": "rt-one"},
            "mcpOAuth": {"server": {"refreshToken": "current"}},
        })
    );
}

const API_KEY: &str = "sk-ant-api03-a1b2c3d4e5a1b2c3d4e5a1b2c3d4e5a1b2c3d4e5";

#[test]
fn api_key_live_state_activates_target_verbatim() {
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "mcpOAuth": {"server": {"refreshToken": "stale"}},
    })
    .to_string();
    assert_eq!(
        Switcher::prepare_credentials_for_activation(&target, Some(API_KEY)),
        target
    );
}

#[test]
fn absent_live_state_activates_target_verbatim() {
    let target = json!({
        "claudeAiOauth": {"accessToken": "target"},
        "mcpOAuth": {"server": {"refreshToken": "old"}},
    })
    .to_string();
    assert_eq!(
        Switcher::prepare_credentials_for_activation(&target, Some("")),
        target
    );
    assert_eq!(
        Switcher::prepare_credentials_for_activation(&target, None),
        target
    );
}

#[test]
fn api_key_target_is_never_composed() {
    let live = json!({
        "claudeAiOauth": {"accessToken": "live"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
    })
    .to_string();
    assert_eq!(
        Switcher::prepare_credentials_for_activation(API_KEY, Some(&live)),
        API_KEY
    );
}

#[test]
fn opaque_target_credential_activates_verbatim() {
    let target = json!({"accessToken": "legacy", "refreshToken": "legacy-rt"}).to_string();
    let live = json!({
        "claudeAiOauth": {"accessToken": "live"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
    })
    .to_string();
    assert_eq!(
        Switcher::prepare_credentials_for_activation(&target, Some(&live)),
        target
    );
}

#[test]
fn normal_switch_preserves_live_shared_state() {
    let h = two_accounts(&mut sample_sequence_data());
    let live = json!({
        "claudeAiOauth": {"accessToken": "sk-live-1", "refreshToken": "rt-live-1"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
    })
    .to_string();
    let target = json!({
        "claudeAiOauth": {"accessToken": "sk-target-2", "refreshToken": "rt-target-2"},
        "mcpOAuth": {"server": {"refreshToken": "stale"}},
    })
    .to_string();
    h.set_backup("1", "test@example.com", &live);
    h.set_backup("2", "account2@example.com", &target);
    h.set_live(&live);
    h.switcher.perform_switch("2", None).unwrap();
    let activated: Value = serde_json::from_str(&h.live().unwrap()).unwrap();
    assert_eq!(activated["claudeAiOauth"]["accessToken"], "sk-target-2");
    assert_eq!(
        activated["mcpOAuth"],
        json!({"server": {"refreshToken": "current"}})
    );
}

#[test]
fn direct_activation_preserves_live_shared_state() {
    let (h, _) = setup(Some("untracked@example.com"));
    let live = json!({
        "claudeAiOauth": {"accessToken": "sk-unmanaged", "refreshToken": "rt-unmanaged"},
        "mcpOAuth": {"server": {"refreshToken": "current"}},
    });
    std::fs::write(h.credentials_file(), live.to_string()).unwrap();
    let mut target: Value =
        serde_json::from_str(&h.backup("1", "one@example.com").unwrap()).unwrap();
    target["mcpOAuth"] = json!({"server": {"refreshToken": "stale"}});
    h.switcher
        .store
        .write_account_credentials("1", "one@example.com", &target.to_string())
        .unwrap();
    h.switcher.perform_switch("1", None).unwrap();
    let activated = live_file(&h);
    assert_eq!(activated["claudeAiOauth"]["accessToken"], "sk-one");
    assert_eq!(
        activated["mcpOAuth"],
        json!({"server": {"refreshToken": "current"}})
    );
}

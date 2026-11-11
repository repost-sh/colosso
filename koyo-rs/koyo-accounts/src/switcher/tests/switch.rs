//! cswap's `tests/test_switcher.py` (0fe22a8) `TestPerformSwitchPostDisplay`: `_perform_switch`
//! on its normal and direct paths, its refusals and the direct activation's rollback.
//! `test_switch_persists_rotated_refresh_token_to_backup`, `test_switch_survives_post_display_
//! failure` and `test_switch_followup_*` are left out with the post-switch display.

use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::{Harness, oauth_creds, profile, sample_sequence_data, tmp_files_in};
use crate::AccountsError;
use crate::switcher::store::WRITE_FAULT;

/// cswap's `_setup_two_accounts`: slots 1 (test@, active) and 2 (account2@, an expired backup
/// and its config); the live credential is each test's.
pub(super) fn two_accounts(data: &mut Value) -> Harness {
    let h = Harness::new();
    h.mock_claude_config();
    data["accounts"]["1"]["email"] = "test@example.com".into();
    h.write_sequence(data);
    h.set_backup("2", "account2@example.com", &expired_2());
    h.set_config_backup(
        "2",
        "account2@example.com",
        &json!({"oauthAccount": {"emailAddress": "account2@example.com", "accountUuid": "uuid-2"}}),
    );
    h
}

pub(super) fn expired_2() -> String {
    json!({"claudeAiOauth": {
        "accessToken": "sk-stale-2", "refreshToken": "rt-orig-2", "expiresAt": 0,
        "scopes": ["user:profile"],
    }})
    .to_string()
}

#[test]
fn switch_refuses_to_overwrite_backup_with_empty_current_creds() {
    let h = two_accounts(&mut sample_sequence_data());
    let good = oauth_creds("sk-good-1", "rt-good-1");
    h.set_backup("1", "test@example.com", &good);
    // No live item and no file: the read is "", as a `security` timeout's.
    assert!(matches!(
        h.switcher.perform_switch("2", None),
        Err(AccountsError::CredentialRead(_))
    ));
    assert_eq!(h.backup("1", "test@example.com").unwrap(), good);
}

fn untracked_config() -> Value {
    json!({"oauthAccount": {
        "emailAddress": "untracked@example.com", "accountUuid": "",
        "organizationUuid": null, "organizationName": null,
    }})
}

fn one_target_slot(h: &Harness, target_creds: &str) {
    h.write_sequence(&json!({
        "activeAccountNumber": null, "lastUpdated": "2024-01-01T00:00:00Z", "sequence": [1],
        "accounts": {"1": {
            "email": "target@example.com", "uuid": "", "organizationUuid": "",
            "organizationName": "", "added": "2024-01-01T00:00:00Z",
        }},
    }));
    h.set_backup("1", "target@example.com", target_creds);
    h.set_config_backup(
        "1",
        "target@example.com",
        &json!({"oauthAccount": {
            "emailAddress": "target@example.com", "accountUuid": "",
            "organizationUuid": null, "organizationName": null,
        }}),
    );
}

#[test]
fn switch_with_unset_active_account_does_not_write_none_backup() {
    let h = Harness::new();
    h.mock_claude_config();
    let target = json!({"claudeAiOauth": {
        "accessToken": "target-token", "refreshToken": null, "expiresAt": null,
        "scopes": ["user:inference"], "subscriptionType": null, "rateLimitTier": null,
    }});
    one_target_slot(&h, &target.to_string());
    h.set_live(&oauth_creds("existing-live-token", "existing-refresh"));
    h.switcher.perform_switch("1", None).unwrap();
    assert!(
        !h.backup_accounts()
            .iter()
            .any(|account| account.starts_with("account-None-"))
    );
    assert!(
        !h.sandbox
            .store()
            .join("configs/.claude-config-None-test@example.com.json")
            .exists()
    );
    assert_eq!(h.live_access_token(), "target-token");
    assert_eq!(h.sequence()["activeAccountNumber"], 1);
}

#[test]
fn switch_uses_live_identity_for_current_backup_slot() {
    // A stale `activeAccountNumber` never decides where the live credential is backed up.
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "realiti44@gmail.com", "accountUuid": "",
        "organizationUuid": null, "organizationName": null,
    }}));
    let record = |email: &str| {
        json!({
            "email": email, "uuid": "", "organizationUuid": "", "organizationName": "",
            "added": "2024-01-01T00:00:00Z",
        })
    };
    h.write_sequence(&json!({
        "activeAccountNumber": 3, "lastUpdated": "2024-01-01T00:00:00Z", "sequence": [3, 4],
        "accounts": {"3": record("onurcetinkol@gmail.com"), "4": record("realiti44@gmail.com")},
    }));
    let live = oauth_creds("realiti-live-token", "realiti-live-refresh");
    h.set_backup(
        "3",
        "onurcetinkol@gmail.com",
        &oauth_creds("target-token", "target-refresh"),
    );
    h.set_backup("4", "realiti44@gmail.com", "old-realiti-backup");
    h.set_config_backup(
        "3",
        "onurcetinkol@gmail.com",
        &json!({"oauthAccount": {"emailAddress": "onurcetinkol@gmail.com", "accountUuid": ""}}),
    );
    h.switcher.setup_directories().unwrap();
    h.switcher
        .write_account_config("4", "realiti44@gmail.com", "old-realiti-config")
        .unwrap();
    h.set_live(&live);
    // cswap's oracle answers a uuid-less identity, which Koyo's takes for no answer: both
    // reach the backup of the live identity's slot.
    h.queue_profile(&profile("", Some("realiti44@gmail.com"), Some("")));
    h.switcher.perform_switch("3", None).unwrap();
    assert_eq!(h.backup("4", "realiti44@gmail.com").unwrap(), live);
    assert!(h.backup("3", "realiti44@gmail.com").is_none());
    assert_eq!(h.live_access_token(), "target-token");
}

/// An untracked login and one managed slot, switched to with the sequence write failing after
/// the live config was replaced; `on_sequence_write` gets the live config's path and runs just
/// before the failure.
fn direct_activation_failing(
    on_sequence_write: impl Fn(&std::path::Path) + 'static,
) -> (Harness, String, String) {
    let h = Harness::new();
    let original_config = untracked_config().to_string();
    std::fs::write(h.config_path(), &original_config).unwrap();
    one_target_slot(&h, &oauth_creds("target-token", "target-refresh"));
    let live = oauth_creds("live-untracked-token", "live-untracked-refresh");
    h.set_live(&live);
    let sequence = h.switcher.sequence_file.clone();
    let config = h.config_path();
    WRITE_FAULT.set(Some(Box::new(move |path, data| {
        (path == sequence && data.get("activeAccountNumber") == Some(&Value::from(1))).then(|| {
            on_sequence_write(&config);
            std::io::Error::other("disk full")
        })
    })));
    let result = h.switcher.perform_switch("1", None);
    WRITE_FAULT.set(None);
    crate::fsutil::FAULT.set(None);
    let Err(error) = result else {
        panic!("the injected failure must fail the switch");
    };
    assert!(error.to_string().contains("disk full"), "{error}");
    (h, original_config, live)
}

#[test]
fn direct_activation_rolls_back_live_creds_on_sequence_write_failure() {
    let (h, original_config, live) = direct_activation_failing(|_| {});
    assert_eq!(h.live().unwrap(), live);
    assert_eq!(
        std::fs::read_to_string(h.config_path()).unwrap(),
        original_config
    );
}

#[test]
fn direct_activation_rollback_publishes_config_by_rename() {
    // Never rewritten in place: a running Claude Code watching it would see an empty file.
    let seen = Arc::new(Mutex::new(None));
    let seen_in = seen.clone();
    let (h, original_config, _) = direct_activation_failing(move |config| {
        *seen_in.lock().unwrap() = Some(std::fs::metadata(config).unwrap().ino());
    });
    let meta = std::fs::metadata(h.config_path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(h.config_path()).unwrap(),
        original_config
    );
    assert_ne!(Some(meta.ino()), *seen.lock().unwrap());
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    assert!(!tmp_files_in(&h.home()));
}

#[test]
fn direct_activation_rollback_failure_leaves_config_intact() {
    // A restore that can't publish leaves the complete post-switch config, never a torn one.
    let (h, _, _) = direct_activation_failing(|_| {
        crate::fsutil::FAULT.set(Some((crate::fsutil::Step::Rename, libc::ENOSPC)));
    });
    assert_eq!(
        h.config()["oauthAccount"]["emailAddress"],
        "target@example.com"
    );
    assert!(!tmp_files_in(&h.home()));
}

#[test]
fn direct_activation_fails_fast_when_live_creds_unreadable() {
    // A live credential that can't be snapshotted is never overwritten.
    let h = Harness::new();
    let original_config = untracked_config().to_string();
    std::fs::write(h.config_path(), &original_config).unwrap();
    one_target_slot(&h, &oauth_creds("target-token", "target-refresh"));
    let file = h.credentials_file();
    std::fs::write(&file, "live-creds-that-we-cannot-read").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = h.switcher.perform_switch("1", None);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let Err(AccountsError::CredentialRead(why)) = result else {
        panic!("an unreadable live credential must fail the activation: {result:?}");
    };
    assert!(why.contains("snapshot"), "{why}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "live-creds-that-we-cannot-read"
    );
    assert!(h.live().is_none());
    assert_eq!(
        std::fs::read_to_string(h.config_path()).unwrap(),
        original_config
    );
}

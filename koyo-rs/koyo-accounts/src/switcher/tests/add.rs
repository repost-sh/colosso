//! cswap's `tests/test_switcher.py` (0fe22a8) for `add_account`: `TestAddAccountRefresh`,
//! `TestAddAccountOrgFields`, `TestAddAccountSlot.test_add_without_slot_auto_assigns`, and the
//! capture guard's `TestStoreResolutionParity` cases (`test_a_secure_store_miss_…`,
//! `test_refuse_degraded_capture_…`), with `tests/test_macos_keychain_contract.py`'s
//! `test_the_default_capture_path_refuses_a_degraded_read`.

use serde_json::json;

use super::{Harness, access_creds};
use crate::AccountsError;
use crate::paths::CLAUDE_CODE_KEYCHAIN_SERVICE;
use crate::testing::Answer;

#[test]
fn readd_existing_account_updates_credentials() {
    let h = Harness::new();
    h.mock_claude_config();
    h.set_live(&access_creds("old-token"));
    let first = h.switcher.add_account().unwrap();
    assert!(!first.updated);
    let accounts = h.accounts();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts["1"]["email"], "test@example.com");
    assert!(
        h.backup("1", "test@example.com")
            .unwrap()
            .contains("old-token")
    );

    h.set_live(&access_creds("new-token"));
    let second = h.switcher.add_account().unwrap();
    assert_eq!(h.accounts().len(), 1);
    assert_eq!(h.sequence()["sequence"].as_array().unwrap().len(), 1);
    // cswap prints "Updated credentials".
    assert!(second.updated);
    assert!(
        h.backup("1", "test@example.com")
            .unwrap()
            .contains("new-token")
    );
}

#[test]
fn allows_same_email_different_org() {
    let h = Harness::new();
    h.set_live(&access_creds("test-token"));
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "user@example.com", "accountUuid": "user-uuid",
        "organizationUuid": "org-uuid-A", "organizationName": "Acme",
    }}));
    h.switcher.add_account().unwrap();
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "user@example.com", "accountUuid": "user-uuid",
    }}));
    h.switcher.add_account().unwrap();
    let accounts = h.accounts();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts["1"]["organizationUuid"], "org-uuid-A");
    assert_eq!(accounts["2"]["organizationUuid"], "");
}

#[test]
fn blocks_true_duplicate() {
    let h = Harness::new();
    h.set_live(&access_creds("test-token"));
    let org_config = json!({"oauthAccount": {
        "emailAddress": "user@example.com", "accountUuid": "user-uuid",
        "organizationUuid": "org-uuid-A", "organizationName": "Acme",
    }});
    h.write_config(&org_config);
    h.switcher.add_account().unwrap();
    h.write_config(&org_config);
    assert!(h.switcher.add_account().unwrap().updated);
    assert_eq!(h.accounts().len(), 1);
}

#[test]
fn stores_org_name_in_sequence() {
    let h = Harness::new();
    h.set_live(&access_creds("test-token"));
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "user@example.com", "accountUuid": "user-uuid",
        "organizationUuid": "org-uuid", "organizationName": "My Org",
    }}));
    let added = h.switcher.add_account().unwrap();
    assert_eq!(added.organization_name, "My Org");
    let accounts = h.accounts();
    assert_eq!(accounts["1"]["organizationName"], "My Org");
    assert_eq!(accounts["1"]["organizationUuid"], "org-uuid");
}

#[test]
fn add_without_slot_auto_assigns() {
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "test@example.com", "accountUuid": "uuid-test@example.com",
        "organizationUuid": "", "organizationName": "",
    }}));
    h.set_live(&access_creds("tok"));
    let added = h.switcher.add_account().unwrap();
    assert_eq!(added.account_num, "1");
    let data = h.sequence();
    assert!(data["accounts"].get("1").is_some());
    assert_eq!(data["activeAccountNumber"], 1);
    assert_eq!(data["sequence"], json!([1]));
}

#[test]
fn add_with_no_live_login_raises() {
    let h = Harness::new();
    let Err(AccountsError::Config(why)) = h.switcher.add_account() else {
        panic!("an add with no live login must be refused");
    };
    assert!(why.contains("log in first"), "{why}");
}

#[test]
fn a_secure_store_miss_does_not_capture_the_other_profiles_key() {
    // The API-key tail follows `CLAUDE_CONFIG_DIR`; a miss in the secure store (a logged-out
    // environment to Claude Code) must not capture the other profile's key.
    let mut h = Harness::new();
    let active = h.home().join("profileA");
    let secure = h.home().join("profileB");
    std::fs::create_dir_all(&active).unwrap();
    std::fs::create_dir_all(&secure).unwrap();
    std::fs::write(
        active.join(".claude.json"),
        json!({"primaryApiKey": "sk-ant-api-PROFILE-A"}).to_string(),
    )
    .unwrap();
    h.sandbox.vars.claude_config_dir = Some(active.to_str().unwrap().into());
    h.sandbox.vars.securestorage_config_dir = Some(secure.to_str().unwrap().into());
    h.rebuild();
    h.switcher.setup_directories().unwrap();
    assert_ne!(
        h.switcher.read_capture_credentials().unwrap().as_deref(),
        Some("sk-ant-api-PROFILE-A")
    );
}

#[test]
fn refuse_degraded_capture_is_not_a_toctou() {
    // The guard's check and the captured value are one read: exactly one Keychain read, and
    // its healthy value, never the stale file a second read could fall to.
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    std::fs::write(h.credentials_file(), "STALE-FALLBACK-PLAINTEXT").unwrap();
    let healthy = r#"{"claudeAiOauth":{"refreshToken":"HEALTHY-RT"}}"#;
    h.set_live(healthy);
    h.sandbox.security.clear_calls();
    assert_eq!(
        h.switcher.read_capture_credentials().unwrap().as_deref(),
        Some(healthy)
    );
    let reads = h
        .sandbox
        .security
        .calls()
        .iter()
        .filter(|call| {
            call.args
                .first()
                .is_some_and(|arg| arg == "find-generic-password")
        })
        .count();
    assert_eq!(reads, 1);
}

#[test]
fn refuse_degraded_capture_control_persistently_locked() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    std::fs::write(h.credentials_file(), "STALE-FALLBACK-PLAINTEXT").unwrap();
    h.sandbox.security.answer_all(Answer::Exit(51));
    assert!(matches!(
        h.switcher.read_capture_credentials(),
        Err(AccountsError::CredentialRead(_))
    ));
}

#[test]
fn the_default_capture_path_refuses_a_degraded_read() {
    let h = Harness::new();
    h.switcher.store.set_keychain_usable_cache(Some(true));
    h.sandbox.security.answer_all(Answer::Exit(51));
    std::fs::write(
        h.credentials_file(),
        json!({"claudeAiOauth": {"accessToken": "sk-maybe-spent"}}).to_string(),
    )
    .unwrap();
    let Err(AccountsError::CredentialRead(why)) = h.switcher.read_capture_credentials() else {
        panic!("a degraded read must not be captured");
    };
    assert!(why.contains("superseded generation"), "{why}");
}

#[test]
fn a_degraded_add_writes_nothing() {
    // The same refusal through `add_account`: no slot, no backup.
    let h = Harness::new();
    h.mock_claude_config();
    std::fs::write(
        h.credentials_file(),
        json!({"claudeAiOauth": {"accessToken": "sk-maybe-spent"}}).to_string(),
    )
    .unwrap();
    h.sandbox
        .security
        .answer_service(CLAUDE_CODE_KEYCHAIN_SERVICE, Answer::Exit(51));
    assert!(matches!(
        h.switcher.add_account(),
        Err(AccountsError::CredentialRead(_))
    ));
    assert!(h.accounts().is_empty());
    assert!(h.backup_accounts().is_empty());
}

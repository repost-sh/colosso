//! cswap's `tests/test_switcher.py` (0fe22a8) for the identity helpers:
//! `TestGetCurrentAccount`, `TestGetCurrentAccountOrgSupport`, `TestGetClaudeConfigPathUtf8`,
//! `TestAccountExists`, `TestAccountExistsCompositeKey`, `TestResolveAccountIdentifier` and
//! `TestResolveIdentifierAmbiguity`; and the rules of `backfill_account_uuid` and
//! `_lineage_key` as their docstrings state them (cswap tests them through the refresh gate
//! and the collector, not ported here).

use serde_json::json;

use super::{Harness, sample_sequence_data};
use crate::AccountsError;
use crate::switcher::{LineageKey, SlotIdentity, locked};

#[test]
fn no_config_file() {
    let h = Harness::new();
    assert_eq!(h.switcher.get_current_account(), None);
}

#[test]
fn with_valid_config() {
    let h = Harness::new();
    h.mock_claude_config();
    assert_eq!(
        h.switcher.get_current_account(),
        Some(("test@example.com".into(), String::new()))
    );
}

#[test]
fn config_without_oauth() {
    let h = Harness::new();
    h.write_config(&json!({"other": "data"}));
    assert_eq!(h.switcher.get_current_account(), None);
}

#[test]
fn config_with_empty_email() {
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {"emailAddress": "", "accountUuid": "uuid"}}));
    assert_eq!(h.switcher.get_current_account(), None);
}

#[test]
fn returns_org_info() {
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "user@example.com", "accountUuid": "user-uuid-1234",
        "organizationUuid": "org-uuid-5678", "organizationName": "Acme Corp",
        "organizationRole": "primary_owner", "displayName": "Test User",
    }}));
    assert_eq!(
        h.switcher.get_current_account(),
        Some(("user@example.com".into(), "org-uuid-5678".into()))
    );
}

#[test]
fn returns_empty_org_for_personal() {
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "user@example.com", "accountUuid": "user-uuid-1234",
    }}));
    assert_eq!(
        h.switcher.get_current_account(),
        Some(("user@example.com".into(), String::new()))
    );
}

#[test]
fn returns_none_when_no_config() {
    assert_eq!(Harness::new().switcher.get_current_account(), None);
}

#[test]
fn fallback_config_with_unicode_punctuation() {
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "user@example.com", "accountUuid": "uuid-1",
        "displayName": "Name with \u{201c}smart\u{201d} quotes",
    }}));
    assert_eq!(h.switcher.claude_config_path(), h.config_path());
    assert_eq!(
        h.switcher.get_current_account(),
        Some(("user@example.com".into(), String::new()))
    );
}

#[test]
fn account_exists() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    assert!(
        h.switcher
            .account_exists("account1@example.com", "")
            .unwrap()
    );
    assert!(
        !h.switcher
            .account_exists("nonexistent@example.com", "")
            .unwrap()
    );
}

#[test]
fn no_sequence_file() {
    assert!(
        !Harness::new()
            .switcher
            .account_exists("any@example.com", "")
            .unwrap()
    );
}

#[test]
fn distinguishes_org_and_personal() {
    let h = Harness::new();
    h.write_sequence(&json!({
        "activeAccountNumber": 1, "lastUpdated": "2024-01-01T00:00:00Z", "sequence": [1],
        "accounts": {"1": {
            "email": "user@example.com", "uuid": "user-uuid", "organizationUuid": "org-uuid-A",
            "organizationName": "Acme", "added": "2024-01-01T00:00:00Z",
        }},
    }));
    assert!(
        h.switcher
            .account_exists("user@example.com", "org-uuid-A")
            .unwrap()
    );
    assert!(!h.switcher.account_exists("user@example.com", "").unwrap());
    assert!(
        !h.switcher
            .account_exists("user@example.com", "org-uuid-B")
            .unwrap()
    );
}

fn resolve(h: &Harness, identifier: &str) -> Option<String> {
    h.switcher.resolve_account_identifier(identifier).unwrap()
}

#[test]
fn resolve_by_number() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    assert_eq!(resolve(&h, "1").as_deref(), Some("1"));
    assert_eq!(resolve(&h, "2").as_deref(), Some("2"));
}

#[test]
fn resolve_by_email() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    assert_eq!(resolve(&h, "account1@example.com").as_deref(), Some("1"));
    assert_eq!(resolve(&h, "account2@example.com").as_deref(), Some("2"));
}

#[test]
fn resolve_nonexistent() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    assert_eq!(resolve(&h, "nonexistent@example.com"), None);
    // Numbers pass through.
    assert_eq!(resolve(&h, "999").as_deref(), Some("999"));
}

fn sample_sequence_data_with_org() -> serde_json::Value {
    json!({
        "activeAccountNumber": 1, "lastUpdated": "2024-01-01T00:00:00Z", "sequence": [1, 2],
        "accounts": {
            "1": {"email": "user@example.com", "uuid": "user-uuid", "organizationUuid": "org-uuid-5678",
                  "organizationName": "Acme Corp", "added": "2024-01-01T00:00:00Z"},
            "2": {"email": "user@example.com", "uuid": "user-uuid", "organizationUuid": "",
                  "organizationName": "", "added": "2024-01-02T00:00:00Z"},
        },
    })
}

#[test]
fn by_number_always_works() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data_with_org());
    assert_eq!(resolve(&h, "1").as_deref(), Some("1"));
    assert_eq!(resolve(&h, "2").as_deref(), Some("2"));
}

#[test]
fn raises_on_ambiguous_email() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data_with_org());
    let Err(AccountsError::Config(why)) = h.switcher.resolve_account_identifier("user@example.com")
    else {
        panic!("an ambiguous email must be refused");
    };
    assert!(why.contains("ambiguous"), "{why}");
}

#[test]
fn unique_email_still_works() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    assert_eq!(resolve(&h, "account1@example.com").as_deref(), Some("1"));
}

fn identity_slots(h: &Harness, uuid_1: &str) {
    h.write_sequence(&json!({
        "activeAccountNumber": 1, "lastUpdated": "2024-01-01T00:00:00Z", "sequence": [1, 2],
        "accounts": {
            "1": {"email": "a@example.com", "uuid": uuid_1, "organizationUuid": "org-1"},
            "2": {"email": "b@example.com", "uuid": "uuid-2", "organizationUuid": ""},
        },
    }));
}

#[test]
fn backfill_fills_only_an_empty_uuid_of_the_expected_identity() {
    let h = Harness::new();
    identity_slots(&h, "");
    // Another identity in the gap: nothing stamped.
    h.switcher
        .backfill_account_uuid("1", "uuid-x", Some("other@example.com"), None)
        .unwrap();
    h.switcher
        .backfill_account_uuid("1", "uuid-x", None, Some(""))
        .unwrap();
    assert_eq!(h.accounts()["1"]["uuid"], "");
    h.switcher
        .backfill_account_uuid("1", "uuid-1", Some("a@example.com"), Some("org-1"))
        .unwrap();
    assert_eq!(h.accounts()["1"]["uuid"], "uuid-1");
    // An existing uuid is identity: never rewritten.
    h.switcher
        .backfill_account_uuid("2", "uuid-x", None, None)
        .unwrap();
    assert_eq!(h.accounts()["2"]["uuid"], "uuid-2");
}

#[test]
fn a_lineage_key_binds_the_slots_full_identity() {
    let h = Harness::new();
    identity_slots(&h, "uuid-1");
    let key = h.switcher.lineage_key("1", "a@example.com", "fp").unwrap();
    assert_eq!(
        key,
        LineageKey {
            slot: "1".into(),
            caller_email: "a@example.com".into(),
            identity: SlotIdentity {
                email: "a@example.com".into(),
                organization_uuid: "org-1".into(),
                uuid: "uuid-1".into(),
            },
            fingerprint: "fp".into(),
        }
    );
    locked(&h.switcher.probe_verdicts).insert(key.clone(), false);
    // The slot re-created for another account misses the verdict.
    identity_slots(&h, "uuid-other");
    let rebuilt = h.switcher.lineage_key("1", "a@example.com", "fp").unwrap();
    assert_eq!(h.switcher.probe_verdict(&key), Some(false));
    assert_eq!(h.switcher.probe_verdict(&rebuilt), None);
}

//! cswap's `tests/test_switcher.py` (0fe22a8) `TestProvenanceGuard`, its fail-open half: a
//! partial or failed answer, a cached verdict, wiped and OAuth-less live credentials, bytes
//! that moved; and `TestUuidConflictClassification`.

use serde_json::json;

use super::provenance::{a1_backup, guarded, run_switch, warned};
use super::switch::two_accounts;
use super::{Harness, profile, sample_sequence_data};
use crate::locked;
use crate::oauth::credential_fingerprint;
use crate::switcher::switch::{Outgoing, OutgoingKind, Provenance};
use crate::testing::creds;
use crate::{AccountsError, oauth};

#[test]
fn partial_identity_uuid_only_matching_outgoing_slot_backs_up() {
    // Partial data never turns a legitimate rotation into preserve-and-skip.
    let rotated = creds("sk-f", "rt-1-rotated", None);
    let h = guarded(&rotated);
    let op = run_switch(&h, Some(profile("uuid-1", None, None)));
    assert_eq!(h.backup("1", "test@example.com").unwrap(), rotated);
    assert!(h.stash().is_empty());
    assert!(op.warnings.is_empty());
}

#[test]
fn partial_identity_uuid_match_with_slot_org_recorded() {
    let mut data = sample_sequence_data();
    data["accounts"]["1"]["organizationUuid"] = "org-1".into();
    let h = two_accounts(&mut data);
    h.set_backup("1", "test@example.com", &a1_backup());
    let roster = h.switcher.get_sequence_data().unwrap().unwrap();
    let rotated = creds("sk-f", "rt-1-rotated", None);
    let provenance = Provenance {
        live: Some(rotated.clone()),
        resolved: Some(oauth::AccountIdentity {
            uuid: "uuid-1".into(),
            email: None,
            organization_uuid: None,
        }),
    };
    let outgoing = Outgoing {
        account: "1",
        email: "test@example.com",
        provenance: &provenance,
    };
    let kind = h
        .switcher
        .classify_outgoing_credential(&outgoing, &rotated, &roster)
        .unwrap();
    assert_eq!(kind, OutgoingKind::OwnRotated);
}

#[test]
fn partial_identity_matching_nothing_falls_open() {
    // Indistinguishable from schema drift: the pre-fix backup, never preserve-and-skip.
    let mystery = creds("sk-x", "rt-x", None);
    let h = guarded(&mystery);
    let op = run_switch(&h, Some(profile("uuid-nobody", None, None)));
    assert_eq!(h.backup("1", "test@example.com").unwrap(), mystery);
    assert!(h.stash().is_empty());
    assert!(op.warnings.is_empty());
}

#[test]
fn foreign_attribution_survives_missing_email() {
    let h = guarded(&creds("sk-2-rotated", "rt-2-rotated", None));
    let a2 = h.backup("2", "account2@example.com").unwrap();
    let op = run_switch(&h, Some(profile("uuid-2", None, Some(""))));
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.backup("2", "account2@example.com").unwrap(), a2);
    assert_eq!(h.stash().len(), 1);
    assert!(warned(&op, "Account-2"));
}

#[test]
fn unresolvable_mismatch_backs_up_pre_fix() {
    // The fail-open core: the oracle silent, the switch behaves exactly pre-fix, quietly.
    let mystery = creds("sk-x", "rt-x", None);
    let h = guarded(&mystery);
    let op = run_switch(&h, None);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), mystery);
    assert!(h.stash().is_empty());
    assert!(op.warnings.is_empty());
    assert_eq!(h.live_access_token(), "sk-stale-2");
}

#[test]
fn cached_foreign_verdict_survives_a_failed_switch_time_probe() {
    // A collect pass already condemned this lineage: a transient probe failure must not let
    // the fail-open backup write it into the slot.
    let foreign = creds("sk-foreign", "rt-foreign", None);
    let h = guarded(&foreign);
    let key = h
        .switcher
        .lineage_key(
            "1",
            "test@example.com",
            &credential_fingerprint(&foreign).unwrap(),
        )
        .unwrap();
    // A verdict as the collect pass's probe records it.
    locked(&h.switcher.probe_verdicts).insert(key, false);
    let op = run_switch(&h, None);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    let stash = h.stash();
    assert_eq!(stash.len(), 1);
    assert_eq!(stash.values().next().unwrap()["reason"], "known-foreign");
    assert!(warned(&op, "previously identified"));
    assert_eq!(h.live_access_token(), "sk-stale-2");
}

#[test]
fn profile_exception_falls_back_to_pre_fix_backup() {
    // A failing lookup (here a 500) is indistinguishable from no answer.
    let mystery = creds("sk-x", "rt-x", None);
    let h = guarded(&mystery);
    h.sandbox
        .http
        .queue_json(oauth::PROFILE_URL, 500, &json!({}));
    let op = h.switcher.perform_switch("2", None).unwrap();
    assert_eq!(h.backup("1", "test@example.com").unwrap(), mystery);
    assert!(h.stash().is_empty());
    assert!(op.warnings.is_empty());
}

#[test]
fn safety_copy_failure_aborts_before_live_overwrite() {
    // No safety copy, no switch.
    let mystery = creds("sk-x", "rt-x", None);
    let h = guarded(&mystery);
    let stash_dir = h.sandbox.store().join("credentials");
    std::fs::set_permissions(
        &stash_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o500),
    )
    .unwrap();
    h.queue_profile(&profile(
        "uuid-unmanaged",
        Some("elsewhere@example.com"),
        Some(""),
    ));
    let result = h.switcher.perform_switch("2", None);
    std::fs::set_permissions(
        &stash_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    assert!(result.is_err());
    assert_eq!(h.live().unwrap(), mystery);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
}

#[test]
fn wiped_live_credential_never_overwrites_a_token_bearing_backup() {
    let wiped = json!({"claudeAiOauth": {
        "accessToken": "", "refreshToken": "", "expiresAt": 1000,
        "scopes": ["user:profile"], "subscriptionType": "max",
    }})
    .to_string();
    let h = guarded(&wiped);
    let op = run_switch(&h, None);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.live_access_token(), "sk-stale-2");
    assert!(h.stash().is_empty());
    assert!(
        op.warnings
            .iter()
            .any(|w| w.to_lowercase().contains("log in"))
    );
}

#[test]
fn wiped_live_matching_wiped_backup_stays_quiet() {
    let wiped =
        json!({"claudeAiOauth": {"accessToken": "", "refreshToken": "", "expiresAt": 1000}})
            .to_string();
    let h = guarded(&wiped);
    h.set_backup("1", "test@example.com", &wiped);
    let op = run_switch(&h, None);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), wiped);
    assert!(op.warnings.is_empty());
}

#[test]
fn oauth_absent_live_never_overwrites_a_token_bearing_backup() {
    // #383: a store left holding only `mcpOAuth`.
    let mcp_only = json!({"mcpOAuth": {"some-server|abc123": {"serverName": "some-server", "accessToken": ""}}})
        .to_string();
    let h = guarded(&mcp_only);
    let op = run_switch(&h, None);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.live_access_token(), "sk-stale-2");
    assert!(h.stash().is_empty());
    assert!(
        op.warnings
            .iter()
            .any(|w| w.to_lowercase().contains("log in"))
    );
}

#[test]
fn oauth_slot_holding_a_key_still_captures_its_own_login() {
    // A slot's kind, not its bytes, decides what it stores.
    let h = guarded(&creds("sk-new-1", "rt-new-1", None));
    h.set_backup("1", "test@example.com", "sk-ant-api03-key");
    run_switch(
        &h,
        Some(profile("uuid-1", Some("test@example.com"), Some(""))),
    );
    assert_eq!(
        h.backup("1", "test@example.com").unwrap(),
        creds("sk-new-1", "rt-new-1", None)
    );
}

#[test]
fn moved_bytes_between_prefetch_and_lock_fall_to_unresolved() {
    // A pre-lock answer binds only to the bytes it resolved.
    let moved = creds("sk-m", "rt-m", None);
    let h = guarded(&moved);
    let provenance = Provenance {
        live: Some("something-else-entirely".into()),
        resolved: Some(oauth::AccountIdentity {
            uuid: "uuid-2".into(),
            email: Some("account2@example.com".into()),
            organization_uuid: Some(String::new()),
        }),
    };
    let op = h.switcher.perform_switch("2", Some(provenance)).unwrap();
    assert_eq!(h.backup("1", "test@example.com").unwrap(), moved);
    assert!(h.stash().is_empty());
    assert!(op.warnings.is_empty());
}

#[test]
fn email_match_with_conflicting_uuid_is_not_the_slot() {
    // A recycled email under another uuid is never backed into the slot.
    let h = guarded(&creds("sk-x", "rt-x", None));
    h.set_backup("1", "test@example.com", &creds("sk-1", "rt-1", None));
    let op = run_switch(
        &h,
        Some(profile(
            "uuid-recycled-email",
            Some("test@example.com"),
            Some(""),
        )),
    );
    assert_eq!(
        h.backup("1", "test@example.com").unwrap(),
        creds("sk-1", "rt-1", None)
    );
    assert_eq!(h.stash().len(), 1);
    assert!(warned(&op, "does not match a managed account"));
}

#[test]
fn a_switch_with_nothing_managed_is_refused() {
    let h = Harness::new();
    assert!(matches!(
        h.switcher.switch_to("1"),
        Err(AccountsError::Config(_))
    ));
}

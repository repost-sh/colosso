//! cswap's `tests/test_switcher.py` (0fe22a8) `TestProvenanceGuard`, its attributing half: the
//! outgoing credential's own lineage, a foreign or alien owner, and the foreign-successor
//! rescue (#489). The fail-open half is in `fallback.rs`. Left out with API-key accounts:
//! `test_api_key_slot_*` and `test_mcp_only_live_never_overwrites_an_api_key_backup` (the
//! `api-key-mismatch` verdict).

use serde_json::{Value, json};

use super::switch::two_accounts;
use super::{Harness, oauth_creds, profile, sample_sequence_data};
use crate::switcher::switch::SwitchOp;

pub(super) fn a1_backup() -> String {
    oauth_creds("sk-stored-1", "rt-1")
}

/// Two accounts, slot 1's backup [`a1_backup`], the live credential `live`.
pub(super) fn guarded(live: &str) -> Harness {
    guarded_with(&mut sample_sequence_data(), live)
}

pub(super) fn guarded_with(data: &mut Value, live: &str) -> Harness {
    let h = two_accounts(data);
    h.set_backup("1", "test@example.com", &a1_backup());
    h.set_live(live);
    h
}

/// cswap's `_run_switch`: slot 1 to 2, the oracle answering `resolver` (or nothing).
pub(super) fn run_switch(h: &Harness, resolver: Option<Value>) -> SwitchOp {
    if let Some(resolver) = resolver {
        h.queue_profile(&resolver);
    }
    h.switcher.perform_switch("2", None).unwrap()
}

fn slot2() -> Value {
    profile("uuid-2", Some("account2@example.com"), Some(""))
}

pub(super) fn warned(op: &SwitchOp, text: &str) -> bool {
    op.warnings.iter().any(|warning| warning.contains(text))
}

#[test]
fn byte_identical_live_skips_credential_backup() {
    let h = guarded(&a1_backup());
    h.sandbox.security.clear_calls();
    let op = run_switch(&h, None);
    let backup_writes = h
        .sandbox
        .security
        .calls()
        .iter()
        .filter(|call| {
            call.stdin.as_deref().is_some_and(|line| {
                line.starts_with("add-generic-password")
                    && line.contains("account-1-test@example.com")
            })
        })
        .count();
    assert_eq!(backup_writes, 0);
    assert!(op.warnings.is_empty());
    assert!(!h.config_backup("1", "test@example.com").is_empty());
}

#[test]
fn access_token_rotation_same_lineage_backs_up() {
    let rotated = json!({"claudeAiOauth": {
        "accessToken": "sk-fresh-1", "refreshToken": "rt-1", "expiresAt": 9,
    }})
    .to_string();
    let h = guarded(&rotated);
    let op = run_switch(&h, None);
    assert_eq!(h.profile_lookups(), 0);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), rotated);
    assert!(op.warnings.is_empty());
}

#[test]
fn full_rotation_resolved_to_outgoing_slot_backs_up() {
    let rotated = oauth_creds("sk-fresh-1", "rt-1-rotated");
    let h = guarded(&rotated);
    let op = run_switch(
        &h,
        Some(profile("uuid-1", Some("test@example.com"), Some(""))),
    );
    assert_eq!(h.backup("1", "test@example.com").unwrap(), rotated);
    assert!(op.warnings.is_empty());
    assert!(h.stash().is_empty());
}

#[test]
fn resolution_backfills_empty_slot_uuid() {
    let mut data = sample_sequence_data();
    data["accounts"]["1"]["uuid"] = "".into();
    let h = guarded_with(&mut data, &oauth_creds("sk-f", "rt-1-rotated"));
    run_switch(
        &h,
        Some(profile("uuid-resolved", Some("test@example.com"), Some(""))),
    );
    assert_eq!(h.accounts()["1"]["uuid"], "uuid-resolved");
}

#[test]
fn foreign_credential_preserved_never_backed_into_any_slot() {
    // The poisoning precondition: another managed slot's bytes, uuid-positive. Preserved,
    // written into neither slot, and the target's stored backup still activates.
    let foreign = oauth_creds("sk-2-rotated", "rt-2-rotated");
    let h = guarded(&foreign);
    let a2 = h.backup("2", "account2@example.com").unwrap();
    let op = run_switch(&h, Some(slot2()));
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.backup("2", "account2@example.com").unwrap(), a2);
    let id = h.only_stash_id();
    assert_eq!(h.safety_copy(&id), foreign);
    assert_eq!(h.stash()[&id]["resolvedIdentity"]["uuid"], "uuid-2");
    assert!(
        op.warnings
            .iter()
            .any(|w| w.contains("ownership mismatch") && w.contains("Account-2"))
    );
    assert_eq!(h.live_access_token(), "sk-stale-2");
}

#[test]
fn foreign_synced_lineage_warns_without_any_write() {
    let h = two_accounts(&mut sample_sequence_data());
    h.set_backup("1", "test@example.com", &a1_backup());
    let a2 = h.backup("2", "account2@example.com").unwrap();
    h.set_live(&a2);
    let op = run_switch(&h, Some(slot2()));
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.backup("2", "account2@example.com").unwrap(), a2);
    assert!(h.stash().is_empty());
    assert!(warned(&op, "already matches Account-2"));
}

fn oauth_at(access: &str, refresh: Option<&str>, expires_at: u64) -> String {
    let mut blob = json!({"accessToken": access, "expiresAt": expires_at});
    if let Some(refresh) = refresh {
        blob["refreshToken"] = refresh.into();
    }
    json!({"claudeAiOauth": blob}).to_string()
}

#[test]
fn rotation_under_a_stale_label_advances_the_owning_slot() {
    // #489: Claude Code rotated slot 3's token while the config named slot 1.
    let mut data = sample_sequence_data();
    data["accounts"]["3"] =
        json!({"email": "account3@example.com", "uuid": "uuid-3", "added": "2024-01-03T00:00:00Z"});
    data["sequence"] = json!([1, 2, 3]);
    let successor = oauth_at("sk-3-new", Some("rt-3-new"), 2_000);
    let h = guarded_with(&mut data, &successor);
    let a2 = h.backup("2", "account2@example.com").unwrap();
    h.set_backup(
        "3",
        "account3@example.com",
        &oauth_at("sk-3-old", Some("rt-3-old"), 1_000),
    );
    let op = run_switch(
        &h,
        Some(profile("uuid-3", Some("account3@example.com"), Some(""))),
    );
    assert_eq!(h.backup("3", "account3@example.com").unwrap(), successor);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.backup("2", "account2@example.com").unwrap(), a2);
    assert_eq!(h.safety_copy(&h.only_stash_id()), successor);
    assert!(warned(&op, "Account-3's backup was updated"));
}

#[test]
fn newer_foreign_generation_of_the_target_is_what_activates() {
    // The target is read after the rescue, so the newer generation activates.
    let successor = oauth_at("sk-2-new", Some("rt-2-new"), 2_000);
    let h = guarded(&successor);
    h.set_backup(
        "2",
        "account2@example.com",
        &oauth_at("sk-2-old", Some("rt-2-old"), 1_000),
    );
    run_switch(&h, Some(slot2()));
    assert_eq!(h.backup("2", "account2@example.com").unwrap(), successor);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.live_access_token(), "sk-2-new");
}

#[test]
fn foreign_bytes_that_are_not_a_newer_full_pair_are_never_written() {
    for (live_refresh, live_exp, why) in [
        (Some("rt-2-other"), 1_000, "same issue time"),
        (Some("rt-2-other"), 500, "older generation"),
        (None, 2_000, "no refresh token"),
    ] {
        let h = guarded(&oauth_at("sk-2-live", live_refresh, live_exp));
        let stored = oauth_at("sk-2-stored", Some("rt-2-stored"), 1_000);
        h.set_backup("2", "account2@example.com", &stored);
        let op = run_switch(&h, Some(slot2()));
        assert_eq!(
            h.backup("2", "account2@example.com").unwrap(),
            stored,
            "{why}"
        );
        assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
        assert_eq!(h.stash().len(), 1);
        assert!(
            op.warnings
                .iter()
                .any(|w| w.contains("ownership mismatch") && w.contains("Account-2"))
        );
    }
}

#[test]
fn stored_login_without_a_comparable_expiry_is_never_replaced() {
    for (stored_login, why) in [
        (
            json!({"accessToken": "sk-ant-oat01-setup", "scopes": ["user:inference"]}),
            "setup token",
        ),
        (
            json!({"accessToken": "sk-2-stored", "refreshToken": "rt-2-stored"}),
            "no expiry",
        ),
        (
            json!({"accessToken": "sk-2-stored", "refreshToken": "rt-2-stored", "expiresAt": "soon"}),
            "unreadable expiry",
        ),
    ] {
        let live = oauth_at("sk-2-live", Some("rt-2-live"), 2_000);
        let h = guarded(&live);
        let stored = json!({"claudeAiOauth": stored_login}).to_string();
        h.set_backup("2", "account2@example.com", &stored);
        let op = run_switch(&h, Some(slot2()));
        assert_eq!(
            h.backup("2", "account2@example.com").unwrap(),
            stored,
            "{why}"
        );
        assert_eq!(h.safety_copy(&h.only_stash_id()), live);
        assert!(warned(&op, "ownership mismatch"));
    }
}

#[test]
fn backup_holding_no_login_is_still_advanced() {
    // A backup with no login in it (#383's MCP-only blob) has nothing to lose.
    let live = oauth_at("sk-2-live", Some("rt-2-live"), 2_000);
    let h = guarded(&live);
    h.set_backup(
        "2",
        "account2@example.com",
        &json!({"mcpOAuth": {"some-server|abc123": {"serverName": "some-server"}}}).to_string(),
    );
    run_switch(&h, Some(slot2()));
    assert_eq!(h.backup("2", "account2@example.com").unwrap(), live);
}

#[test]
fn alien_credential_preserved_and_skipped() {
    let alien = oauth_creds("sk-x", "rt-x");
    let h = guarded(&alien);
    let op = run_switch(
        &h,
        Some(profile(
            "uuid-unmanaged",
            Some("elsewhere@example.com"),
            Some(""),
        )),
    );
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.safety_copy(&h.only_stash_id()), alien);
    assert!(warned(&op, "does not match a managed account"));
}

#[test]
fn blank_stored_uuid_email_match_is_alien_not_foreign() {
    // A cross-slot attribution must be uuid-positive.
    let mut data = sample_sequence_data();
    data["accounts"]["2"]["uuid"] = "".into();
    let h = guarded_with(&mut data, &oauth_creds("sk-d", "rt-d"));
    let a2 = h.backup("2", "account2@example.com").unwrap();
    let op = run_switch(
        &h,
        Some(profile(
            "uuid-2-real",
            Some("account2@example.com"),
            Some(""),
        )),
    );
    assert_eq!(h.backup("1", "test@example.com").unwrap(), a1_backup());
    assert_eq!(h.backup("2", "account2@example.com").unwrap(), a2);
    assert_eq!(h.stash().len(), 1);
    assert!(warned(&op, "does not match a managed account"));
}

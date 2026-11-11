//! cswap's `tests/test_switcher.py` (0fe22a8) `TestActiveAccountRefresh` cases of the
//! rotated-backup resync on the fast path (`_resync_rotated_backup`) and its ownership probe
//! (`_resolved_matches_slot_identity`, the verdict cache, `_lineage_key`). cswap patches the
//! live read, the backup read and the backup write; here the live credential is Claude Code's
//! Keychain item, the backup the slot's item (set again before each pass, as cswap's patched
//! read returns it every time), and a write that landed is the backup's new value.

use serde_json::{Value, json};

use super::support::{EMAIL, call_text, creds, new, posted, queue_usage};
use super::usage_active::{active_fetch, active_harness};
use super::{Harness, profile};
use crate::credentials::ActiveVerdict;
use crate::locked;
use crate::oauth::{PROFILE_URL, USAGE_FOREIGN_CREDENTIAL};
use crate::paths::CLAUDE_CODE_KEYCHAIN_SERVICE;
use crate::usage_store::FetchRecord;

fn expired() -> String {
    creds("sk-active", "rt-orig", Some(json!(1000)))
}

/// cswap's `_PROFILE_SELF`.
fn profile_self() -> Value {
    profile("uuid-1", Some(EMAIL), None)
}

/// cswap's `_fresh_drift_pass`: live `REFRESHED` (cswap's `_NEW`), backup `EXPIRED`, the
/// oracle answering `probe` (nothing: unresolvable), and the usage endpoint
/// `{"five_hour": 3%}`.
fn fresh_drift_pass(h: &Harness, probe: Option<&Value>) -> FetchRecord {
    h.set_live(&new());
    h.set_backup("1", EMAIL, &expired());
    if let Some(body) = probe {
        h.sandbox.http.queue_json(PROFILE_URL, 200, body);
    }
    queue_usage(h, &json!({"five_hour": {"utilization": 3}}));
    active_fetch(h, "1", EMAIL, &new(), ActiveVerdict::default())
}

fn without_uuid(h: &Harness) {
    let mut data = h.sequence();
    data["accounts"]["1"]
        .as_object_mut()
        .unwrap()
        .remove("uuid");
    h.write_sequence(&data);
}

fn resynced(h: &Harness) -> bool {
    h.backup("1", EMAIL).unwrap() == new()
}

#[test]
fn fresh_fetch_resyncs_backup_after_external_rotation() {
    let h = active_harness();
    let first = fresh_drift_pass(&h, Some(&profile_self()));
    assert_eq!(first.usage, Some(json!({"five_hour": {"pct": 3}})));
    assert!(resynced(&h));
    // The verdict is kept: the next drift resyncs without asking the oracle again.
    fresh_drift_pass(&h, None);
    assert!(resynced(&h));
    assert_eq!(h.profile_lookups(), 1);
    assert!(posted(&h).is_empty());
}

#[test]
fn fresh_fetch_same_lineage_skips_the_resync() {
    let h = active_harness();
    h.set_backup("1", EMAIL, &new());
    h.sandbox.security.clear_calls();
    queue_usage(&h, &json!({"five_hour": {"utilization": 3}}));
    active_fetch(&h, "1", EMAIL, &new(), ActiveVerdict::default());
    let calls: Vec<String> = h.sandbox.security.calls().iter().map(call_text).collect();
    assert!(
        !calls
            .iter()
            .any(|call| call.contains(CLAUDE_CODE_KEYCHAIN_SERVICE)),
        "the live store was read"
    );
    assert!(
        !calls
            .iter()
            .any(|call| call.starts_with("add-generic-password")),
        "the backup was written"
    );
}

#[test]
fn fresh_foreign_probe_mismatch_skips_resync_warns_once_and_caches() {
    let h = active_harness();
    let foreign = profile("uuid-foreign", Some("other@example.com"), None);
    let first = fresh_drift_pass(&h, Some(&foreign));
    let second = fresh_drift_pass(&h, Some(&foreign));
    assert_eq!(first.sentinel.as_deref(), Some(USAGE_FOREIGN_CREDENTIAL));
    assert_eq!(first.usage, None);
    assert_eq!(second.sentinel.as_deref(), Some(USAGE_FOREIGN_CREDENTIAL));
    assert!(!resynced(&h));
    assert_eq!(h.profile_lookups(), 1, "the foreign verdict is kept");
    // The warning's once-only mark.
    assert!(locked(&h.switcher.provenance_warned).contains(&("1".into(), EMAIL.into())));
}

#[test]
fn fresh_probe_failure_skips_resync_and_retries_next_pass() {
    let h = active_harness();
    let first = fresh_drift_pass(&h, None);
    assert_eq!(first.usage, Some(json!({"five_hour": {"pct": 3}})));
    assert!(!resynced(&h));
    assert_eq!(h.profile_lookups(), 1);
    fresh_drift_pass(&h, Some(&profile_self()));
    assert_eq!(h.profile_lookups(), 2, "a failure isn't kept");
    assert!(resynced(&h));
}

#[test]
fn fresh_probe_unverifiable_not_cached() {
    let h = active_harness();
    without_uuid(&h);
    let partial = profile("uuid-x", None, None);
    fresh_drift_pass(&h, Some(&partial));
    assert!(!resynced(&h));
    fresh_drift_pass(&h, Some(&partial));
    assert!(!resynced(&h));
    assert_eq!(h.profile_lookups(), 2);
    assert!(locked(&h.switcher.probe_verdicts).is_empty());
}

#[test]
fn uuid_less_slot_email_match_with_missing_org_is_unverifiable() {
    let h = active_harness();
    without_uuid(&h);
    let orgless = profile("uuid-x", Some(EMAIL), None);
    let first = fresh_drift_pass(&h, Some(&orgless));
    assert!(!resynced(&h));
    assert!(locked(&h.switcher.probe_verdicts).is_empty());
    assert_eq!(h.switcher.account_identity("1").unwrap().uuid, "");
    assert_eq!(first.usage, Some(json!({"five_hour": {"pct": 3}})));
}

#[test]
fn uuid_less_personal_slot_rejects_same_email_under_foreign_org() {
    let h = active_harness();
    without_uuid(&h);
    let sibling = profile("uuid-org-sibling", Some(EMAIL), Some("org-B"));
    let first = fresh_drift_pass(&h, Some(&sibling));
    fresh_drift_pass(&h, Some(&sibling));
    assert!(!resynced(&h));
    assert_eq!(first.sentinel.as_deref(), Some(USAGE_FOREIGN_CREDENTIAL));
    assert_eq!(h.profile_lookups(), 1, "the condemning verdict is kept");
    assert_eq!(h.switcher.account_identity("1").unwrap().uuid, "");
}

#[test]
fn uuid_less_slot_exact_email_org_match_resyncs_and_backfills() {
    let h = active_harness();
    without_uuid(&h);
    let complete = profile("uuid-resolved", Some(EMAIL), Some(""));
    let first = fresh_drift_pass(&h, Some(&complete));
    assert_eq!(first.usage, Some(json!({"five_hour": {"pct": 3}})));
    assert!(resynced(&h));
    assert_eq!(
        h.switcher.account_identity("1").unwrap().uuid,
        "uuid-resolved"
    );
}

#[test]
fn lineage_key_binds_the_stored_email_too() {
    let h = active_harness();
    without_uuid(&h);
    let before = h.switcher.lineage_key("1", EMAIL, "fp").unwrap();
    let mut data = h.sequence();
    data["accounts"]["1"]["email"] = "new@example.com".into();
    h.write_sequence(&data);
    assert_ne!(h.switcher.lineage_key("1", EMAIL, "fp").unwrap(), before);
}

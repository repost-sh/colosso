//! From claude-swap's `tests/test_usage_store.py` (0fe22a8):
//! `TestIdentityGuard`, `TestClaims`, `TestSentinels`, `TestFreshness`, `TestPollPlan`,
//! `TestClaimTrustBridge`.

use std::collections::BTreeMap;

use super::*;

// TestIdentityGuard

#[test]
fn slot_reuse_hides_old_usage() {
    let h = Harness::new();
    h.record("1", ok());
    let rebound = identity("1", "new@x.com", "");
    assert_eq!(h.entry_as("1", &rebound), UsageEntry::default());
}

#[test]
fn same_email_different_org_is_a_different_account() {
    let h = Harness::new();
    h.record("1", ok());
    let rebound = identity("1", "a@x.com", "org-9");
    assert_eq!(h.entry_as("1", &rebound), UsageEntry::default());
}

#[test]
fn write_replaces_mismatched_row() {
    let h = Harness::new();
    h.record("1", ok());
    let rebound = identity("1", "new@x.com", "");
    h.record_as("1", err("timeout"), &rebound);
    let entry = h.entry_as("1", &rebound);
    assert_eq!(entry.last_good, None);
    assert_eq!(entry.consecutive_failures, 1);
}

#[test]
fn untouched_slots_survive_subset_writes() {
    let h = Harness::new();
    let both = BTreeMap::from([("1".to_owned(), ok()), ("2".to_owned(), ok())]);
    h.store.record(&both, &ident(), None, None).unwrap();
    h.record_as("1", err("timeout"), &identity("1", "a@x.com", ""));
    assert_eq!(h.entry("2").last_good, Some(usage()));
}

// TestClaims

#[test]
fn claim_marks_in_flight() {
    let h = Harness::new();
    let claims = h.store.claim(&["1"], &ident()).unwrap();
    assert_eq!(won(&claims), ["1"]);
    assert_eq!(claims["1"].len(), 32);
    assert!(h.entry("1").claimed(h.now()));
    h.advance(CLAIM_TTL_S + 1.);
    assert!(!h.entry("1").claimed(h.now()));
}

#[test]
fn legacy_last_attempt_claim_is_honored_during_schema_overlap() {
    let h = Harness::new();
    h.store.claim(&["1"], &ident()).unwrap();
    let mut raw = h.raw();
    let row = raw["accounts"]["1"].as_object_mut().unwrap();
    row.remove("claimId");
    row.remove("claimUntil");
    h.write_raw(&raw.to_string());

    assert!(h.entry("1").claimed(h.now()));
    assert!(h.reserve(&["1"], true).is_empty());
    h.advance(11.);
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
}

#[test]
fn claim_does_not_touch_measurement() {
    let h = Harness::new();
    h.record("1", ok());
    h.advance(100.);
    h.store.claim(&["1"], &ident()).unwrap();
    let entry = h.entry("1");
    assert_eq!(entry.last_good, Some(usage()));
    assert_eq!(entry.age_s, Some(100.));
}

#[test]
fn live_claim_outlasts_urgent_poll_interval() {
    let h = Harness::new();
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
    h.advance(61.);
    assert!(h.reserve(&["1"], false).is_empty());
}

#[test]
fn record_releases_long_claim_immediately() {
    let h = Harness::new();
    let claims = h.reserve(&["1"], true);
    assert!(h.entry("1").claimed(h.now()));
    assert_eq!(h.record_fenced("1", ok(), &claims), set(&["1"]));
    assert!(!h.entry("1").claimed(h.now()));
}

#[test]
fn failure_releases_claim() {
    let h = Harness::new();
    let claims = h.reserve(&["1"], true);
    assert_eq!(h.record_fenced("1", err("timeout"), &claims), set(&["1"]));
    let entry = h.entry("1");
    assert!(!entry.claimed(h.now()));
    assert_eq!(entry.last_error.as_deref(), Some("timeout"));
}

#[test]
fn sentinel_releases_claim_without_persisting_state() {
    let h = Harness::new();
    let claims = h.reserve(&["1"], true);
    let claimed_at = h.entry("1").last_attempt_at;
    h.advance(1.);
    let sentinel = FetchRecord::sentinel("token expired");
    assert_eq!(h.record_fenced("1", sentinel, &claims), set(&["1"]));
    let entry = h.entry("1");
    assert!(!entry.claimed(h.now()));
    assert_eq!(entry.last_attempt_at, claimed_at);
    assert_eq!(entry.sentinel, None);
    assert_eq!(entry.last_good, None);
}

#[test]
fn expired_writer_cannot_clear_or_overwrite_new_lease() {
    let h = Harness::new();
    let first = h.reserve(&["1"], true);
    h.advance(CLAIM_TTL_S + 1.);
    let second = h.reserve(&["1"], true);
    assert_ne!(first["1"], second["1"]);

    assert!(h.record_fenced("1", err("timeout"), &first).is_empty());
    let entry = h.entry("1");
    assert!(entry.claimed(h.now()));
    assert_eq!(entry.last_error, None);

    assert_eq!(h.record_fenced("1", ok(), &second), set(&["1"]));
    assert_eq!(h.entry("1").last_good, Some(usage()));
}

#[test]
fn stale_writer_cannot_replace_a_rebound_identity() {
    let h = Harness::new();
    let stale_claim = h.reserve(&["1"], true);
    let rebound = identity("1", "new@x.com", "org-new");
    let rewon = h.store.reserve(&["1"], &rebound, true, false).unwrap();
    assert_eq!(won(&rewon), ["1"]);

    assert!(h.record_fenced("1", ok(), &stale_claim).is_empty());
    let entry = h.entry_as("1", &rebound);
    assert!(entry.claimed(h.now()));
    assert_eq!(entry.last_good, None);
}

#[test]
fn partial_records_can_reuse_their_explicit_claims() {
    let h = Harness::new();
    let claims = h.reserve(&["1", "2"], true);
    assert_eq!(h.record_fenced("1", ok(), &claims), set(&["1"]));
    assert!(h.entry("2").claimed(h.now()));
    assert_eq!(h.record_fenced("2", ok(), &claims), set(&["2"]));
    assert_eq!(h.entry("1").last_good, Some(usage()));
    assert_eq!(h.entry("2").last_good, Some(usage()));
}

#[test]
fn mixed_record_accepts_only_the_current_claim() {
    let h = Harness::new();
    let first = h.reserve(&["1", "2"], true);
    h.advance(CLAIM_TTL_S + 1.);
    let second = h.reserve(&["1"], true);
    assert_ne!(first["1"], second["1"]);

    let outcomes = BTreeMap::from([("1".to_owned(), err("timeout")), ("2".to_owned(), ok())]);
    let accepted = h
        .store
        .record(&outcomes, &ident(), Some(&first), None)
        .unwrap();
    assert_eq!(accepted, set(&["2"]));
    assert_eq!(h.entry("1").last_error, None);
    assert!(h.entry("1").claimed(h.now()));
    assert_eq!(h.entry("2").last_good, Some(usage()));
}

#[test]
fn unfenced_record_cannot_overwrite_a_live_claim() {
    let h = Harness::new();
    let claims = h.reserve(&["1"], true);
    assert!(h.record("1", err("timeout")).is_empty());
    let entry = h.entry("1");
    assert!(entry.claimed(h.now()));
    assert_eq!(entry.last_error, None);
    assert_eq!(h.record_fenced("1", ok(), &claims), set(&["1"]));
}

#[test]
fn unfenced_record_accepts_after_a_claim_expires() {
    let h = Harness::new();
    h.reserve(&["1"], true);
    h.advance(CLAIM_TTL_S + 1.);
    assert_eq!(h.record("1", ok()), set(&["1"]));
    let entry = h.entry("1");
    assert_eq!(entry.last_good, Some(usage()));
    assert!(!entry.claimed(h.now()));
}

#[test]
fn credential_refresh_revokes_an_old_fetch_claim() {
    let h = Harness::new();
    let claims = h.reserve(&["1"], true);
    h.store.clear_dead_token(&["1"], &ident()).unwrap();
    assert!(
        h.record_fenced("1", err("invalid_grant"), &claims)
            .is_empty()
    );
    let entry = h.entry("1");
    assert!(!entry.claimed(h.now()));
    assert!(!entry.token_dead(None));
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
}

#[test]
fn success_commits_its_new_plan_without_a_duplicate_window() {
    let h = Harness::new();
    h.record("1", ok());
    h.plan("1", Some(h.now() + 60.), Some(60.));
    h.advance(61.);
    let claims = h.reserve(&["1"], false);
    assert_eq!(won(&claims), ["1"]);

    let next_poll = h.now() + 300.;
    let plans = BTreeMap::from([("1".to_owned(), (Some(next_poll), Some(300.)))]);
    let outcomes = BTreeMap::from([("1".to_owned(), ok())]);
    h.store
        .record(&outcomes, &ident(), Some(&claims), Some(&plans))
        .unwrap();
    let entry = h.entry("1");
    assert_eq!(entry.next_poll_at, Some(next_poll));
    assert_eq!(entry.poll_interval_s, Some(300.));
    assert!(h.reserve(&["1"], false).is_empty());
}

// TestSentinels

#[test]
fn sentinel_record_is_a_store_noop() {
    let h = Harness::new();
    h.record("1", ok());
    h.record("1", FetchRecord::sentinel("token expired"));
    let entry = h.entry("1");
    assert_eq!(entry.sentinel, None);
    assert_eq!(entry.last_good, Some(usage()));
}

#[test]
fn refused_credential_stamp_rides_a_sentinel() {
    let h = Harness::new();
    let stamped = FetchRecord {
        rejected_fp: Some("sha256-at:abc".to_owned()),
        ..FetchRecord::sentinel("token expired")
    };
    h.record("1", stamped);
    assert_eq!(
        h.entry("1").rejected_fingerprint.as_deref(),
        Some("sha256-at:abc")
    );
    h.record("1", FetchRecord::sentinel("token expired"));
    assert_eq!(
        h.entry("1").rejected_fingerprint.as_deref(),
        Some("sha256-at:abc")
    );
    h.record("1", ok());
    assert_eq!(h.entry("1").rejected_fingerprint, None);
}

#[test]
fn overlay_wins_decisions_but_not_display() {
    let h = Harness::new();
    h.record("1", ok());
    let entry = with_sentinel(h.entry("1"), Some("token expired"));
    assert_eq!(
        entry.decision_value(),
        Some(Decision::Sentinel("token expired"))
    );
    assert_eq!(entry.last_good, Some(usage()));
}

#[test]
fn with_sentinel_none_is_identity() {
    let entry = UsageEntry {
        last_good: Some(usage()),
        ..UsageEntry::default()
    };
    assert_eq!(with_sentinel(entry.clone(), None), entry);
}

// TestFreshness

#[test]
fn fresh_within_serve_ttl() {
    let h = Harness::new();
    h.record("1", ok());
    let entry = h.entry("1");
    assert!(entry.fresh(h.now()));
    assert!(entry.fresh(h.now() + SERVE_TTL_S));
    assert!(!entry.fresh(h.now() + SERVE_TTL_S + 1.));
}

// TestPollPlan

#[test]
fn set_and_read_poll_plan() {
    let h = Harness::new();
    h.record("1", ok());
    h.plan("1", Some(h.now() + 120.), Some(120.));
    let entry = h.entry("1");
    assert_eq!(entry.next_poll_at, Some(h.now() + 120.));
    assert_eq!(entry.poll_interval_s, Some(120.));
    assert_eq!(entry.last_good, Some(usage()));
}

#[test]
fn poll_plan_clear() {
    let h = Harness::new();
    h.plan("1", Some(h.now() + 120.), Some(120.));
    h.plan("1", None, None);
    let entry = h.entry("1");
    assert_eq!(entry.next_poll_at, None);
    assert_eq!(entry.poll_interval_s, None);
}

// TestClaimTrustBridge

#[test]
fn in_flight_claim_keeps_decision_trust() {
    let h = Harness::new();
    h.record("1", ok());
    h.plan("1", Some(h.now() + 400.), Some(400.));
    h.advance(401.);
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
    let entry = h.entry("1");
    assert!(entry.trust_extended);
    assert_eq!(decision(&entry), Some(&usage()));
    h.advance(CLAIM_TTL_S);
    assert_eq!(h.entry("1").decision_value(), None);
}

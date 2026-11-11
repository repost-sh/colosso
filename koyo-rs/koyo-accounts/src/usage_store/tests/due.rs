//! From claude-swap's `tests/test_usage_store.py` (0fe22a8):
//! `TestDueCandidate`, `TestDeadTokenQuarantine`, `TestReserve`, `TestFingerprintBoundStrikes`,
//! `TestStruckFingerprintHygiene`.

use std::collections::BTreeMap;

use serde_json::json;

use super::*;

const NOW: f64 = 1_000_000.;

fn entries(rows: Vec<(&str, UsageEntry)>) -> BTreeMap<String, UsageEntry> {
    rows.into_iter()
        .map(|(num, entry)| (num.to_owned(), entry))
        .collect()
}

fn fetched(ago: f64) -> UsageEntry {
    UsageEntry {
        fetched_at: Some(NOW - ago),
        age_s: Some(ago),
        ..UsageEntry::default()
    }
}

// TestDueCandidate

#[test]
fn missing_entry_is_most_due() {
    let table = entries(vec![("3", fetched(60.))]);
    assert_eq!(
        due_candidate(&["2", "3"], &table, NOW).as_deref(),
        Some("2")
    );
}

#[test]
fn never_fetched_beats_fetched() {
    let table = entries(vec![("2", fetched(999.)), ("3", UsageEntry::default())]);
    assert_eq!(
        due_candidate(&["2", "3"], &table, NOW).as_deref(),
        Some("3")
    );
}

#[test]
fn stalest_fetched_wins() {
    let table = entries(vec![("2", fetched(60.)), ("3", fetched(300.))]);
    assert_eq!(
        due_candidate(&["2", "3"], &table, NOW).as_deref(),
        Some("3")
    );
}

#[test]
fn sentinel_accounts_skipped() {
    let table = entries(vec![(
        "2",
        with_sentinel(UsageEntry::default(), Some("api-key")),
    )]);
    assert_eq!(due_candidate(&["2"], &table, NOW), None);
}

#[test]
fn backoff_skipped_until_it_expires() {
    let table = entries(vec![(
        "2",
        UsageEntry {
            backoff_until: Some(NOW + 10.),
            ..UsageEntry::default()
        },
    )]);
    assert_eq!(due_candidate(&["2"], &table, NOW), None);
    assert_eq!(
        due_candidate(&["2"], &table, NOW + 11.).as_deref(),
        Some("2")
    );
}

#[test]
fn future_next_poll_at_skipped() {
    let table = entries(vec![
        (
            "2",
            UsageEntry {
                fetched_at: Some(NOW - 300.),
                next_poll_at: Some(NOW + 60.),
                ..UsageEntry::default()
            },
        ),
        (
            "3",
            UsageEntry {
                fetched_at: Some(NOW - 60.),
                ..UsageEntry::default()
            },
        ),
    ]);
    assert_eq!(
        due_candidate(&["2", "3"], &table, NOW).as_deref(),
        Some("3")
    );
}

fn parked(last_good: Value, next_in: f64, interval: f64) -> UsageEntry {
    UsageEntry {
        last_good: Some(last_good),
        next_poll_at: Some(NOW + next_in),
        poll_interval_s: Some(interval),
        ..fetched(400.)
    }
}

#[test]
fn reset_parked_exhausted_plan_is_due_for_repair() {
    let table = entries(vec![(
        "2",
        parked(json!({"seven_day": {"pct": 100.0}}), 86_400., 300.),
    )]);
    assert_eq!(due_candidate(&["2"], &table, NOW).as_deref(), Some("2"));
}

#[test]
fn bounded_exhausted_plan_is_not_due_early() {
    let table = entries(vec![(
        "2",
        parked(json!({"seven_day": {"pct": 100.0}}), 600., 600.),
    )]);
    assert_eq!(due_candidate(&["2"], &table, NOW), None);
}

#[test]
fn parked_plan_is_repaired_after_scoped_model_is_deselected() {
    let reading = json!({
        "five_hour": {"pct": 10.0},
        "seven_day": {"pct": 10.0},
        "scoped": [{"name": "Fable", "pct": 100.0}],
    });
    let table = entries(vec![("2", parked(reading, 86_400., 300.))]);
    assert_eq!(due_candidate(&["2"], &table, NOW).as_deref(), Some("2"));
}

#[test]
fn attempted_but_unmeasured_slot_yields_to_an_older_due_row() {
    let touched = |fetched_ago: Option<f64>, attempt_ago: f64| UsageEntry {
        fetched_at: fetched_ago.map(|ago| NOW - ago),
        last_attempt_at: Some(NOW - attempt_ago),
        ..UsageEntry::default()
    };
    let mut table = entries(vec![
        ("2", touched(Some(400.), 400.)),
        ("3", touched(None, 60.)),
    ]);
    assert_eq!(
        due_candidate(&["2", "3"], &table, NOW).as_deref(),
        Some("2")
    );
    table.insert("2".to_owned(), touched(Some(30.), 30.));
    assert_eq!(
        due_candidate(&["2", "3"], &table, NOW).as_deref(),
        Some("3")
    );
}

#[test]
fn adopted_reading_counts_as_a_touch() {
    let table = entries(vec![
        (
            "2",
            UsageEntry {
                fetched_at: Some(NOW - 30.),
                last_attempt_at: Some(NOW - 900.),
                ..UsageEntry::default()
            },
        ),
        (
            "3",
            UsageEntry {
                fetched_at: Some(NOW - 600.),
                last_attempt_at: Some(NOW - 600.),
                ..UsageEntry::default()
            },
        ),
    ]);
    assert_eq!(
        due_candidate(&["2", "3"], &table, NOW).as_deref(),
        Some("3")
    );
}

#[test]
fn none_when_no_candidates() {
    let none: [&str; 0] = [];
    assert_eq!(due_candidate(&none, &BTreeMap::new(), NOW), None);
}

// TestDeadTokenQuarantine

#[test]
fn invalid_grant_advances_strikes() {
    let h = Harness::new();
    h.record("1", err("invalid_grant"));
    assert_eq!(h.entry("1").auth_dead_strikes, 1);
    h.record("1", err("invalid_grant"));
    assert_eq!(h.entry("1").auth_dead_strikes, 2);
}

#[test]
fn transient_error_does_not_advance_or_reset() {
    let h = Harness::new();
    h.record("1", err("invalid_grant"));
    h.record("1", err("http-429"));
    assert_eq!(h.entry("1").auth_dead_strikes, 1);
}

#[test]
fn success_resets_strikes() {
    let h = Harness::new();
    h.record("1", err("invalid_grant"));
    h.record("1", err("invalid_grant"));
    h.record("1", ok());
    assert_eq!(h.entry("1").auth_dead_strikes, 0);
}

#[test]
fn token_dead_at_threshold() {
    let h = Harness::new();
    assert!(!h.entry("1").token_dead(None));
    h.record("1", err("invalid_grant"));
    // One server-confirmed invalid_grant is definitive.
    assert!(h.entry("1").token_dead(None));
}

#[test]
fn transient_error_alone_never_marks_dead() {
    let h = Harness::new();
    for _ in 0..5 {
        h.record("1", err("http-429"));
    }
    assert!(!h.entry("1").token_dead(None));
}

#[test]
fn due_candidate_skips_dead_token() {
    let h = Harness::new();
    h.record("1", err("invalid_grant"));
    h.record("1", err("invalid_grant"));
    h.advance(10_000.);
    let table = h.store.entries(&ident(), &[]);
    assert!(table["1"].token_dead(None));
    assert_eq!(due_candidate(&["1"], &table, h.now()), None);
}

#[test]
fn clear_dead_token_lifts_quarantine() {
    let h = Harness::new();
    h.record("1", err("invalid_grant"));
    h.record("1", err("invalid_grant"));
    assert!(h.entry("1").token_dead(None));
    h.store.clear_dead_token(&["1"], &ident()).unwrap();
    let entry = h.entry("1");
    assert_eq!(entry.auth_dead_strikes, 0);
    assert!(!entry.token_dead(None));
    assert_eq!(entry.last_error, None);
    assert_eq!(entry.backoff_until, None);
}

// TestReserve

fn stale(h: &Harness) {
    h.record("1", ok());
    h.advance(SERVE_TTL_S + CLAIM_TTL_S + 1.);
}

#[test]
fn reserve_wins_and_stamps() {
    let h = Harness::new();
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
    // The stamp is the claim: an immediate second reservation loses.
    assert!(h.reserve(&["1"], true).is_empty());
    assert!(h.reserve(&["1"], false).is_empty());
}

#[test]
fn fresh_entry_not_won() {
    let h = Harness::new();
    h.record("1", ok());
    h.advance(CLAIM_TTL_S + 1.);
    assert!(h.reserve(&["1"], true).is_empty());
}

#[test]
fn respect_plans_waits_for_next_poll() {
    let h = Harness::new();
    stale(&h);
    h.plan("1", Some(h.now() + 300.), Some(300.));
    assert!(h.reserve(&["1"], true).is_empty());
    h.advance(301.);
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
}

#[test]
fn overslept_repair_rechecks_current_plan_under_lock() {
    let h = Harness::new();
    stale(&h);
    h.plan("1", Some(h.now() + 86_400.), Some(300.));
    let claims = h.reserve_repair(&["1"], true);
    assert_eq!(won(&claims), ["1"]);
    // A concurrent winner can replace the obsolete plan first; the locked check sees it.
    let plans = BTreeMap::from([("1".to_owned(), (Some(h.now() + 300.), Some(300.)))]);
    let outcomes = BTreeMap::from([("1".to_owned(), ok())]);
    let accepted = h
        .store
        .record(&outcomes, &ident(), Some(&claims), Some(&plans))
        .unwrap();
    assert_eq!(accepted, set(&["1"]));
    h.advance(SERVE_TTL_S + 1.);
    h.plan("1", Some(h.now() + 300.), Some(300.));
    assert!(h.reserve_repair(&["1"], false).is_empty());
}

#[test]
fn scheduler_beats_the_ttl_when_due() {
    let h = Harness::new();
    h.record("1", ok());
    h.plan("1", Some(h.now() + 60.), Some(60.));
    h.advance(61.);
    assert!(h.reserve(&["1"], true).is_empty());
    assert_eq!(won(&h.reserve(&["1"], false)), ["1"]);
}

#[test]
fn scheduler_may_fetch_a_not_due_stale_entry() {
    let h = Harness::new();
    stale(&h);
    h.plan("1", Some(h.now() + 600.), Some(600.));
    assert_eq!(won(&h.reserve(&["1"], false)), ["1"]);
}

#[test]
fn backoff_blocks_both_modes() {
    let h = Harness::new();
    h.record("1", err("timeout"));
    h.advance(BACKOFF_BASE_S - 1.);
    assert!(h.reserve(&["1"], true).is_empty());
    assert!(h.reserve(&["1"], false).is_empty());
}

#[test]
fn dead_token_never_won() {
    let h = Harness::new();
    h.record("1", err("invalid_grant"));
    h.advance(TRUST_MAX_AGE_S);
    assert!(h.reserve(&["1"], true).is_empty());
    assert!(h.reserve(&["1"], false).is_empty());
}

#[test]
fn unknown_row_and_identity_mismatch_win() {
    let h = Harness::new();
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
    h.record("2", ok());
    let other = identity("2", "new@x.com", "org-9");
    let claims = h.store.reserve(&["2"], &other, true, false).unwrap();
    assert_eq!(won(&claims), ["2"]);
}

// TestFingerprintBoundStrikes

fn struck(fp: Option<&str>) -> UsageEntry {
    let h = Harness::new();
    let ident = identity("1", "a@example.com", "");
    let claims = h.store.reserve(&["1"], &ident, false, false).unwrap();
    let record = FetchRecord {
        struck_fp: fp.map(str::to_owned),
        ..err("invalid_grant")
    };
    let outcomes = BTreeMap::from([("1".to_owned(), record)]);
    h.store
        .record(&outcomes, &ident, Some(&claims), None)
        .unwrap();
    entry_as(&h.store, "1", &ident)
}

#[test]
fn strike_stamps_fingerprint() {
    let entry = struck(Some("fp-A"));
    assert_eq!(entry.auth_dead_strikes, 1);
    assert!(entry.token_dead(Some("fp-A")));
}

#[test]
fn strike_unbinds_on_fingerprint_mismatch() {
    assert!(!struck(Some("fp-A")).token_dead(Some("fp-B")));
}

#[test]
fn strike_without_fp_binds_unconditionally() {
    assert!(struck(None).token_dead(Some("fp-anything")));
}

// TestStruckFingerprintHygiene

fn strike(h: &Harness, fp: Option<&str>) {
    let record = FetchRecord {
        struck_fp: fp.map(str::to_owned),
        ..err("invalid_grant")
    };
    record_as(&h.store, "1", record, &identity("1", "a@b.c", ""));
}

#[test]
fn legacy_strike_overwrites_stale_fingerprint() {
    let h = Harness::new();
    let ident = identity("1", "a@b.c", "");
    strike(&h, Some("sha256:old"));
    h.store.clear_dead_token(&["1"], &ident).unwrap();
    strike(&h, None);
    let entry = entry_as(&h.store, "1", &ident);
    assert_eq!(entry.struck_fingerprint, None);
    assert!(entry.token_dead(Some("sha256:new")));
}

#[test]
fn a_legacy_restrike_overwrites_a_live_stale_fingerprint() {
    let h = Harness::new();
    let ident = identity("1", "a@b.c", "");
    strike(&h, Some("sha256:old"));
    assert_eq!(
        entry_as(&h.store, "1", &ident)
            .struck_fingerprint
            .as_deref(),
        Some("sha256:old")
    );
    strike(&h, None);
    let entry = entry_as(&h.store, "1", &ident);
    assert_eq!(entry.struck_fingerprint, None);
    assert!(entry.token_dead(Some("sha256:new")));
}

#[test]
fn clear_dead_token_drops_fingerprint() {
    let h = Harness::new();
    let ident = identity("1", "a@b.c", "");
    strike(&h, Some("sha256:old"));
    h.store.clear_dead_token(&["1"], &ident).unwrap();
    assert_eq!(entry_as(&h.store, "1", &ident).struck_fingerprint, None);
}

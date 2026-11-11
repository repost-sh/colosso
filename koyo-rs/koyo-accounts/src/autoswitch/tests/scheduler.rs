//! cswap's `TestAdaptiveScheduler`, the engine's half. cswap runs these end to end through the
//! real collector and store and counts fetches; the collector is the switcher's, so here each
//! test hands the engine the store as the collector would have left it and asserts what the
//! engine nominates (`fetch`, `scheduled`). What only the collector and planner decide (cadence
//! decay, urgent mode, the serve TTL, backoff) is theirs to test.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::*;
use crate::oauth::USAGE_TOKEN_EXPIRED;
use crate::poll_policy::{CANDIDATE_DEFAULT_INTERVAL_S, MIN_INTERVAL_S};

/// A stored row: a reading fetched at `fetched_at`, planned for `next_poll_at`.
fn row(value: &Value, fetched_at: f64, plan: Option<(f64, f64)>, now: f64) -> UsageEntry {
    UsageEntry {
        last_good: Some(value.clone()),
        fetched_at: Some(fetched_at),
        age_s: Some(now - fetched_at),
        last_attempt_at: Some(fetched_at),
        next_poll_at: plan.map(|(at, _)| at),
        poll_interval_s: plan.map(|(_, interval)| interval),
        ..UsageEntry::default()
    }
}

fn set(nums: &[&str]) -> BTreeSet<String> {
    nums.iter().map(|num| (*num).to_owned()).collect()
}

/// Ticks on `store` and returns each collection pass's `(fetch, scheduled)`.
fn tick_on(
    h: &mut Harness,
    store: BTreeMap<String, UsageEntry>,
) -> (TickOutcome, Vec<(BTreeSet<String>, bool)>) {
    h.host.lock().fetches.clear();
    let outcome = h.tick_with_entries(store);
    let fetches = h.host.lock().fetches.clone();
    (outcome, fetches)
}

fn entries(rows: Vec<(&str, UsageEntry)>) -> BTreeMap<String, UsageEntry> {
    rows.into_iter()
        .map(|(num, entry)| (num.to_owned(), entry))
        .collect()
}

#[test]
fn baseline_fetches_active_plus_one_candidate() {
    let mut h = Harness::standard();
    let t0 = h.now();
    let (fifty, ten, twenty) = (usage(50.), usage(10.), usage(20.));
    // t0: the active (never fetched) and the stalest candidate.
    let (_, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", UsageEntry::default()),
            ("2", UsageEntry::default()),
            ("3", UsageEntry::default()),
        ]),
    );
    assert_eq!(fetches[1], (set(&["1", "2"]), true));
    // t60: the active planned MIN_INTERVAL_S out; the never-fetched candidate is the due one.
    h.advance(60.);
    let now = h.now();
    let (_, fetches) = tick_on(
        &mut h,
        entries(vec![
            (
                "1",
                row(&fifty, t0, Some((t0 + MIN_INTERVAL_S, MIN_INTERVAL_S)), now),
            ),
            (
                "2",
                row(
                    &ten,
                    t0,
                    Some((
                        t0 + CANDIDATE_DEFAULT_INTERVAL_S,
                        CANDIDATE_DEFAULT_INTERVAL_S,
                    )),
                    now,
                ),
            ),
            ("3", UsageEntry::default()),
        ]),
    );
    assert_eq!(fetches[1].0, set(&["3"]));
    // t120: nobody due, everyone served from the store.
    h.advance(60.);
    let now = h.now();
    let stored = |now: f64| {
        entries(vec![
            (
                "1",
                row(&fifty, t0, Some((t0 + MIN_INTERVAL_S, MIN_INTERVAL_S)), now),
            ),
            (
                "2",
                row(
                    &ten,
                    t0,
                    Some((
                        t0 + CANDIDATE_DEFAULT_INTERVAL_S,
                        CANDIDATE_DEFAULT_INTERVAL_S,
                    )),
                    now,
                ),
            ),
            (
                "3",
                row(
                    &twenty,
                    t0 + 60.,
                    Some((
                        t0 + 60. + CANDIDATE_DEFAULT_INTERVAL_S,
                        CANDIDATE_DEFAULT_INTERVAL_S,
                    )),
                    now,
                ),
            ),
        ])
    };
    let (_, fetches) = tick_on(&mut h, stored(now));
    // Nothing nominated: the read before the nomination is the only pass.
    assert_eq!(fetches, [(set(&[]), false)]);
    // t180: the active account's plan comes due.
    h.advance(60.);
    let now = h.now();
    let (_, fetches) = tick_on(&mut h, stored(now));
    assert_eq!(fetches[1].0, set(&["1"]));
    assert_eq!(fetches.len(), 2, "no escalation at 50%");
}

#[test]
fn near_threshold_escalates_to_full_refresh() {
    // threshold 90, margin 15: the active at 80% is inside the escalation band.
    let mut h = Harness::standard();
    let (outcome, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", entry_for(&usage(80.), START)),
            ("2", entry_for(&usage(10.), START)),
            ("3", entry_for(&usage(20.), START)),
        ]),
    );
    assert_eq!(outcome, TickOutcome::NoAction); // still below the threshold
    assert_eq!(fetches.last().unwrap(), &(set(&["1", "2", "3"]), false)); // everyone refreshed
}

#[test]
fn active_unknown_escalates_before_failover() {
    let mut h = Harness::fleet(
        3,
        AutoSwitchSettings {
            unhealthy_ticks: 1,
            ..defaults()
        },
    );
    let timed_out = UsageEntry {
        last_error: Some("timeout".into()),
        consecutive_failures: 1,
        ..UsageEntry::default()
    };
    let (outcome, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", timed_out),
            ("2", entry_for(&usage(10.), START)),
            ("3", entry_for(&usage(50.), START)),
        ]),
    );
    // The candidates were refreshed in the tick the failover ran on (the pass after it reads
    // the new active row's plan for the wait).
    assert_eq!(fetches[2].0, set(&["1", "2", "3"]));
    assert_eq!(fetches[3], (set(&[]), false));
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn stale_candidate_plan_never_gates_the_active() {
    // A role change outside a switch (a manual login): the active slot carries a plan written
    // while it was an idle candidate, 600 s out. The ACTIVE_MAX_INTERVAL_S age cap overrides it.
    let mut h = Harness::fleet(2, defaults());
    let t0 = h.now();
    let fifty = usage(50.);
    let candidate = |now| row(&usage(20.), now, Some((now + 300., 300.)), now);
    h.advance(240.); // inside the bogus plan, under the age cap
    let now = h.now();
    let (_, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", row(&fifty, t0, Some((t0 + 600., 600.)), now)),
            ("2", candidate(now)),
        ]),
    );
    // The active isn't nominated, nor anyone else: one pass, nothing fetched, the plan kept.
    assert_eq!(fetches, [(set(&[]), false)], "a valid plan is kept");
    h.advance(120.); // age 360 >= ACTIVE_MAX_INTERVAL_S
    let now = h.now();
    let (_, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", row(&fifty, t0, Some((t0 + 600., 600.)), now)),
            ("2", candidate(now)),
        ]),
    );
    assert!(fetches[1].0.contains("1"));
    assert!(!fetches[1].1, "the candidate-style plan is overridden");
}

#[test]
fn engine_repairs_legacy_reset_parked_active_plan() {
    let mut h = Harness::fleet(1, defaults());
    let t0 = h.now();
    let reset_ts = t0 + 86_400.;
    let exhausted = usage_r(100., &iso_at(reset_ts));
    h.advance(400.);
    let now = h.now();
    // A distant reset deadline kept with a much shorter learned interval.
    let (_, fetches) = tick_on(
        &mut h,
        entries(vec![(
            "1",
            row(&exhausted, t0, Some((reset_ts, 300.)), now),
        )]),
    );
    assert!(fetches[1].0.contains("1"));
}

#[test]
fn active_in_backoff_keeps_trusted_headroom() {
    // The active's fetches are refused (429, long Retry-After): its reading ages past
    // STALE_OK_S but stays trusted, so no unhealthy tick and no escalation while rate-limited.
    let mut h = Harness::standard();
    let now = h.now();
    let backed_off = UsageEntry {
        last_error: Some("http-429".into()),
        backoff_until: Some(now + 200.),
        trust_extended: true,
        ..row(&usage(50.), now - 400., None, now)
    };
    let (outcome, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", backed_off),
            (
                "2",
                row(&usage(10.), now - 400., Some((now + 100., 300.)), now),
            ),
            ("3", UsageEntry::default()),
        ]),
    );
    assert_eq!(outcome, TickOutcome::NoAction);
    assert_eq!(h.engine.unhealthy_ticks, 0);
    assert_eq!(fetches.len(), 2, "baseline only, no escalate-all");
}

#[test]
fn all_exhausted_escalation_preserves_wider_plan() {
    let mut h = Harness::standard();
    let now = h.now();
    let exhausted = usage(100.);
    // The wider plan learned after repeated 429s: escalation must not defeat it.
    let (outcome, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", row(&exhausted, now, None, now)),
            (
                "2",
                row(&exhausted, now - 300., Some((now + 1800., 1800.)), now),
            ),
            ("3", row(&exhausted, now - 400., None, now)),
        ]),
    );
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(fetches.last().unwrap().0, set(&["1", "3"]));
}

#[test]
fn idle_hold_skips_candidate_polling() {
    let mut h = Harness::standard();
    let expired = entries(vec![
        ("1", entry_for(&json!(USAGE_TOKEN_EXPIRED), START)),
        ("2", entry_for(&usage(10.), START - 400.)),
        ("3", entry_for(&usage(20.), START - 400.)),
    ]);
    assert_eq!(tick_on(&mut h, expired.clone()).0, TickOutcome::NoAction);
    h.advance(10.);
    // The hold established: no candidate slot is spent.
    let (outcome, fetches) = tick_on(&mut h, expired);
    assert_eq!(outcome, TickOutcome::NoAction);
    assert!(!fetches[1].0.contains("2") && !fetches[1].0.contains("3"));
    assert_eq!(h.reasons().last(), Some(&"active-idle"));
}

#[test]
fn poll_event_carries_fetch_errors() {
    let mut h = Harness::fleet(2, defaults());
    let refused = UsageEntry {
        last_error: Some("http-429".into()),
        ..UsageEntry::default()
    };
    tick_on(
        &mut h,
        entries(vec![("1", refused), ("2", entry_for(&usage(10.), START))]),
    );
    let poll = h.all::<PollEvent>().remove(0);
    assert_eq!(
        poll.fetch_errors.get("1").map(String::as_str),
        Some("http-429")
    );
    assert!(poll.human().contains("http-429"));
}

#[test]
fn quarantined_candidate_never_consumes_the_poll_slot() {
    let mut h = Harness::standard();
    h.quarantine(2, QuarantineReason::InvalidGrant);
    let mut nominated = BTreeSet::new();
    for _ in 0..3 {
        let now = h.now();
        let (_, fetches) = tick_on(
            &mut h,
            entries(vec![
                ("1", row(&usage(50.), now, Some((now + 180., 180.)), now)),
                ("2", UsageEntry::default()),
                ("3", UsageEntry::default()),
            ]),
        );
        nominated.extend(fetches.into_iter().flat_map(|(fetch, _)| fetch));
        h.advance(60.);
    }
    // The alternate slot always went to 3; 2 is dead weight.
    assert!(!nominated.contains("2"));
    assert!(nominated.contains("3"));
}

#[test]
fn disabled_account_polled_after_switchable_candidates_without_escalating() {
    let mut h = Harness::standard();
    h.host.lock().accounts.get_mut(&3).unwrap().disabled = true;
    // t0: the switchable candidate 2 takes the alternate slot.
    let (_, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", UsageEntry::default()),
            ("2", UsageEntry::default()),
            ("3", UsageEntry::default()),
        ]),
    );
    assert_eq!(fetches[1].0, set(&["1", "2"]));
    // t60: 2 isn't due, so the disabled 3 is polled for the panel.
    h.advance(60.);
    let now = h.now();
    let t0 = START;
    let two = row(&usage(10.), t0, Some((t0 + 300., 300.)), now);
    let (_, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", row(&usage(50.), t0, Some((t0 + 180., 180.)), now)),
            ("2", two),
            ("3", UsageEntry::default()),
        ]),
    );
    assert_eq!(fetches[1].0, set(&["3"]));
    // Escalation refreshes switchable candidates only, never disabled ones.
    h.advance(360.);
    let now = h.now();
    let (_, fetches) = tick_on(
        &mut h,
        entries(vec![
            ("1", row(&usage(80.), now, None, now)),
            ("2", row(&usage(10.), t0, Some((t0 + 300., 300.)), now)),
            (
                "3",
                row(&usage(20.), t0 + 60., Some((t0 + 360., 300.)), now),
            ),
        ]),
    );
    assert_eq!(fetches.last().unwrap().0, set(&["1", "2"]));
}

#[test]
fn expired_active_enters_idle_hold_even_during_backoff() {
    // The owned and expired sentinel isn't hidden by the active row's failure backoff, or the
    // engine would count unhealthy ticks toward a spurious failover.
    let mut h = Harness::standard();
    let now = h.now();
    let expired_in_backoff = UsageEntry {
        sentinel: Some(USAGE_TOKEN_EXPIRED.into()),
        last_error: Some("http-429".into()),
        backoff_until: Some(now + 600.),
        ..UsageEntry::default()
    };
    let (outcome, _) = tick_on(
        &mut h,
        entries(vec![
            ("1", expired_in_backoff),
            ("2", entry_for(&usage(10.), now)),
            ("3", entry_for(&usage(20.), now)),
        ]),
    );
    assert_eq!(outcome, TickOutcome::NoAction);
    assert_eq!(h.engine.unhealthy_ticks, 0);
    assert_eq!(h.reasons(), ["active-idle"]);
}

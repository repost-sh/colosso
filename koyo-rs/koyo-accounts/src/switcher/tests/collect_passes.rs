//! cswap's `tests/test_switcher.py` (0fe22a8) for what a collection pass decides:
//! `TestDeadTokenQuarantine`'s cases that stay and `TestPollPolicyInputsCarryTheWeeklyLine`.
//! cswap's patched `_run_usage_fetches` is the `RUN_FETCHES` seam.
//!
//! Adapted: `test_inputs_come_from_the_file_then_the_pin_then_the_file_again` (Koyo has no
//! settings file and no unpin: the inputs are cswap's defaults until pinned), and
//! `test_plans_after_fetch_hands_the_planner_both_lines` (the planner isn't patched: the
//! interval it chose shows which line it was handed, its next poll being jittered).
//! Skipped with the collector's dead-token quarantine, which isn't ported (`usage/collect.rs`
//! says why): `test_collector_surfaces_relogin_sentinel_and_skips_fetch`,
//! `test_relogin_surfaces_same_pass_on_invalid_grant`, `TestHealedStrikeUnblocksFetching`
//! (`test_collector_clears_stale_strike_row`) and `TestStrikeUnbindsInCollector`
//! (`test_relogin_lifts_after_credential_replaced`); with the refresh inside a usage fetch,
//! `TestInactiveRefreshRoutesThroughGate`.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde_json::{Value, json};

use super::Harness;
use super::support::{EMAIL, creds, info};
use crate::Clock as _;
use crate::poll_policy::{MIN_INTERVAL_S, SERVE_TTL_S, URGENT_INTERVAL_S};
use crate::switcher::Fetch;
use crate::switcher::info::AccountInfo;
use crate::switcher::usage::PollInputs;
use crate::testing::{entry_as, iso, record_as};
use crate::usage_store::{FetchRecord, Identities, UsageEntry, one_identity};

/// An `invalid_grant` recorded for `num`'s identity, fenced by the lease it reserved: the
/// strike a re-add must clear.
fn strike(h: &Harness, num: &str, email: &str, org: &str) {
    let identities = one_identity(num, email, org);
    let claims = h
        .switcher
        .usage_store
        .reserve(&[num], &identities, false, false)
        .unwrap();
    h.switcher
        .usage_store
        .record(
            &[(num.to_owned(), FetchRecord::failure("invalid_grant", None))].into(),
            &identities,
            Some(&claims),
            None,
        )
        .unwrap();
}

/// The `RUN_FETCHES` seam answering `records` for this thread's passes, counting its calls.
fn seam(records: BTreeMap<String, FetchRecord>) -> Rc<Cell<usize>> {
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    crate::switcher::usage::RUN_FETCHES.set(Some(Box::new(move |_: &[&AccountInfo]| {
        count.set(count.get() + 1);
        records.clone()
    })));
    calls
}

fn collect(h: &Harness, infos: &[AccountInfo], fetch: Fetch<'_>) -> BTreeMap<String, UsageEntry> {
    h.switcher.collect_usage_entries(infos, fetch).unwrap()
}

/// Rewrites slot 2's stored row.
fn edit_row(h: &Harness, edit: impl FnOnce(&mut Value)) {
    let path = &h.switcher.usage_store.path;
    let mut table: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    edit(&mut table["accounts"]["2"]);
    std::fs::write(path, table.to_string()).unwrap();
}

#[test]
fn the_collector_hands_the_trust_bound_its_configured_models() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    h.switcher
        .set_poll_policy_inputs(90., None, &["Fable".to_owned()]);
    let now = h.sandbox.clock.now();
    let live = creds("at", "rt", Some(json!((now + 86_400.) * 1000.)));
    let infos = [info("2", EMAIL, "", false, &live)];
    let ident: Identities = one_identity("2", EMAIL, "");
    let reading = json!({
        "five_hour": {"pct": 25.0, "resets_at": iso(now + 100. * 3600.)},
        "seven_day": {"pct": 10.0, "resets_at": iso(now + 100. * 3600.)},
        "scoped": [{"name": "Fable", "pct": 90.0, "resets_at": iso(now + 0.5 * 3600.)}],
    });
    record_as(
        &h.switcher.usage_store,
        "2",
        FetchRecord::success(Some(reading)),
        &ident,
    );
    edit_row(&h, |row| {
        row["fetchedAt"] = json!(row["fetchedAt"].as_f64().unwrap() - SERVE_TTL_S * 4.);
    });
    let only = BTreeSet::from(["2".to_owned()]);
    let pass = Fetch::Only {
        only: &only,
        scheduled: false,
    };
    let calls = seam([("2".into(), FetchRecord::failure("http-429", Some(3600.)))].into());
    collect(&h, &infos, pass);
    assert_eq!(calls.get(), 1);
    let models = ["Fable"];
    assert!(
        h.switcher.usage_store.entries(&ident, &models)["2"]
            .backoff_until
            .is_some(),
        "premise: a backoff was recorded"
    );
    edit_row(&h, |row| {
        row["lastGood"]["scoped"][0]["resets_at"] = json!(iso(now - 60.));
        row["fetchedAt"] = json!(row["fetchedAt"].as_f64().unwrap() - 1860.);
        row["backoffUntil"] = Value::Null;
    });
    seam(BTreeMap::new());
    let fetched = collect(&h, &infos, pass).remove("2").unwrap();
    assert_eq!(
        fetched.decision_value(),
        None,
        "the fetching path served last_good although its scoped window has reset"
    );
    let empty = BTreeSet::new();
    let returned = collect(
        &h,
        &infos,
        Fetch::Only {
            only: &empty,
            scheduled: false,
        },
    )
    .remove("2")
    .unwrap();
    crate::switcher::usage::RUN_FETCHES.set(None);
    assert_eq!(
        returned.decision_value(),
        None,
        "the collector never handed its configured models to the bound"
    );
}

#[test]
fn readd_clears_quarantine() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    let identity = one_identity("1", "user@example.com", "org-A");
    strike(&h, "1", "user@example.com", "org-A");
    assert!(entry_as(&h.switcher.usage_store, "1", &identity).token_dead(None));
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "user@example.com", "accountUuid": "u",
        "organizationUuid": "org-A", "organizationName": "Acme",
    }}));
    h.set_live(&super::access_creds("fresh"));
    h.switcher.add_account().unwrap();
    assert!(!entry_as(&h.switcher.usage_store, "1", &identity).token_dead(None));
}

#[test]
fn inputs_come_from_the_file_then_the_pin_then_the_file_again() {
    let h = Harness::new();
    assert_eq!(h.switcher.poll_policy_inputs(), PollInputs::default());
    assert_eq!(
        h.switcher.poll_policy_inputs(),
        PollInputs {
            threshold: 90.,
            weekly_threshold: None,
            models: vec![]
        }
    );
    h.switcher.set_poll_policy_inputs(80., None, &[]);
    assert_eq!(
        h.switcher.poll_policy_inputs(),
        PollInputs {
            threshold: 80.,
            weekly_threshold: None,
            models: vec![]
        }
    );
}

#[test]
fn plans_after_fetch_hands_the_planner_both_lines() {
    // The 7-day window rose from 70 to 75 on the active account: inside the band under the
    // 85 line (urgent cadence), outside the band under a 97 weekly line (halved cadence).
    let h = Harness::new();
    let infos = [info("2", EMAIL, "", true, "")];
    let before = UsageEntry {
        last_good: Some(json!({"five_hour": {"pct": 5.0}, "seven_day": {"pct": 70.0}})),
        poll_interval_s: Some(300.),
        ..UsageEntry::default()
    };
    let pre = BTreeMap::from([("2".to_owned(), before)]);
    let record = FetchRecord::success(Some(
        json!({"five_hour": {"pct": 5.0}, "seven_day": {"pct": 75.0}}),
    ));
    let records = BTreeMap::from([("2".to_owned(), record)]);
    h.switcher
        .set_poll_policy_inputs(85., Some(97.), &["Fable".to_owned()]);
    let inputs = h.switcher.poll_policy_inputs();
    let plans = h
        .switcher
        .plans_after_fetch(&records, &pre, &infos, &inputs);
    assert_eq!(
        plans["2"].1,
        Some(MIN_INTERVAL_S),
        "the weekly line wasn't handed over"
    );
    h.switcher.set_poll_policy_inputs(85., None, &[]);
    let inputs = h.switcher.poll_policy_inputs();
    let plans = h
        .switcher
        .plans_after_fetch(&records, &pre, &infos, &inputs);
    assert_eq!(
        plans["2"].1,
        Some(URGENT_INTERVAL_S),
        "the threshold or is_active wasn't"
    );
    assert!(plans["2"].0.is_some());
}

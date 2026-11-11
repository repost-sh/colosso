//! cswap's `tests/test_subscription.py` (0fe22a8) `TestCollector`: the switcher's
//! subscription plans, `_collect_subscription_plans` and `_plans_for_pass`. cswap patches
//! `subscription.fetch_plan`; here the profile endpoint answers with the organization fields
//! the plan is read from, and the access tokens it was asked with are the calls. The usage
//! store's clock is the sandbox's.
//!
//! Adapted: `test_the_fallback_read_uses_the_display_cache` (no spy on the read's arguments:
//! the backup is replaced behind a primed display cache, and the cached token is the one
//! used), `test_a_response_for_another_account_is_not_kept` (the row's roster slot holds the
//! slot's uuid, which cswap's patched `_get_sequence_data` gives).

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::Harness;
use super::support::{bearers, info};
use crate::Clock as _;
use crate::credentials::backup_username;
use crate::oauth::PROFILE_URL;
use crate::paths::BACKUP_KEYCHAIN_SERVICE;
use crate::subscription::{FAILURE_RETRY_S, PLAN_FILENAME, PLAN_TTL_S, identity_key};
use crate::switcher::Fetch;
use crate::switcher::info::AccountInfo;

const NOW: f64 = 1_000_000.;

fn harness() -> Harness {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    h.sandbox.clock.set(NOW);
    h
}

fn obj(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn team() -> Map<String, Value> {
    obj(
        json!({"tier": "default_claude_max_5x", "organizationType": "claude_team",
               "billingType": "stripe_subscription", "subscriptionStatus": "active",
               "planDisplayName": null}),
    )
}

fn lapsed() -> Map<String, Value> {
    obj(json!({"tier": "default_claude_ai", "billingType": "none",
               "subscriptionStatus": "canceled", "planDisplayName": null}))
}

fn active_max() -> Map<String, Value> {
    obj(
        json!({"tier": "default_claude_max_20x", "billingType": "stripe_subscription",
               "subscriptionStatus": "active", "planDisplayName": null}),
    )
}

/// cswap's `_creds`: an access token alone, expiring at `expires_ms`.
fn token_until(token: &str, expires_ms: f64) -> String {
    json!({"claudeAiOauth": {"accessToken": token, "expiresAt": expires_ms}}).to_string()
}

fn future_ms(h: &Harness) -> f64 {
    (h.sandbox.clock.now() + 3600.) * 1000.
}

fn past_ms(h: &Harness) -> f64 {
    (h.sandbox.clock.now() - 3600.) * 1000.
}

/// The profile's answer for `plan`, read for organization `org` and account `account`
/// (cswap's `_ok`).
fn ok(h: &Harness, plan: &Map<String, Value>, org: &str, account: &str) {
    let mut organization = json!({"uuid": org});
    for (key, source) in [
        ("tier", "rate_limit_tier"),
        ("organizationType", "organization_type"),
        ("billingType", "billing_type"),
        ("subscriptionStatus", "subscription_status"),
        ("planDisplayName", "plan_display_name"),
    ] {
        if let Some(value) = plan.get(key).filter(|value| !value.is_null()) {
            organization[source] = value.clone();
        }
    }
    h.sandbox.http.queue_json(
        PROFILE_URL,
        200,
        &json!({"account": {"uuid": account}, "organization": organization}),
    );
}

/// A failed profile read (cswap's `PlanFetch()`).
fn failed(h: &Harness) {
    h.sandbox.http.queue_json(PROFILE_URL, 500, &json!({}));
}

fn calls(h: &Harness) -> Vec<String> {
    bearers(h, PROFILE_URL)
}

fn active_info(h: &Harness, num: &str) -> AccountInfo {
    info(num, "a@x", "o1", true, &token_until("live", future_ms(h)))
}

fn collect(h: &Harness, infos: &[AccountInfo]) -> Map<String, Value> {
    h.switcher
        .collect_subscription_plans(infos)
        .unwrap()
        .into_iter()
        .collect()
}

#[test]
fn fetches_a_due_slot_and_stores_it() {
    let h = harness();
    ok(&h, &team(), "o1", "");
    let plans = collect(&h, &[active_info(&h, "1")]);
    assert_eq!(calls(&h), ["live"]);
    assert_eq!(plans["1"]["plan"], Value::Object(team()));
    assert_eq!(plans["1"]["fetchedAt"], NOW);
}

#[test]
fn a_record_without_the_name_key_is_refetched_once() {
    let h = harness();
    let infos = [active_info(&h, "1")];
    let key = identity_key("a@x", Some("o1"));
    let mut unnamed = Map::new();
    unnamed.insert(
        key,
        json!({"plan": {"tier": "default_claude_max_5x"}, "fetchedAt": NOW}),
    );
    h.switcher.subscription_store().save(&unnamed).unwrap();
    ok(&h, &team(), "o1", "");
    assert_eq!(collect(&h, &infos)["1"]["plan"], Value::Object(team()));
    collect(&h, &infos);
    assert_eq!(calls(&h), ["live"]);
}

#[test]
fn a_fresh_record_is_served_without_a_call() {
    let h = harness();
    let infos = [active_info(&h, "1")];
    ok(&h, &team(), "o1", "");
    collect(&h, &infos);
    h.sandbox.clock.advance(60.);
    ok(&h, &lapsed(), "o1", "");
    let plans = collect(&h, &infos);
    assert_eq!(calls(&h).len(), 1);
    assert_eq!(plans["1"]["plan"], Value::Object(team()));
}

#[test]
fn a_clock_set_back_does_not_pin_the_old_plan() {
    let h = harness();
    let infos = [active_info(&h, "1")];
    ok(&h, &active_max(), "o1", "");
    collect(&h, &infos);
    h.sandbox.clock.advance(-3600.);
    let infos = [active_info(&h, "1")];
    ok(&h, &lapsed(), "o1", "");
    let plans = collect(&h, &infos);
    assert_eq!(calls(&h).len(), 2);
    assert_eq!(plans["1"]["plan"], Value::Object(lapsed()));
    let plans = collect(&h, &infos);
    assert_eq!(calls(&h).len(), 2, "a reading that landed is not due again");
    assert_eq!(plans["1"]["plan"], Value::Object(lapsed()));
}

#[test]
fn a_failed_fetch_keeps_the_reading_and_backs_off() {
    let h = harness();
    ok(&h, &team(), "o1", "");
    collect(&h, &[active_info(&h, "1")]);
    h.sandbox.clock.advance(PLAN_TTL_S);
    failed(&h);
    let plans = collect(&h, &[active_info(&h, "1")]);
    assert_eq!(plans["1"]["plan"], Value::Object(team()));
    assert_eq!(plans["1"]["fetchedAt"], NOW);
    h.sandbox.clock.advance(60.);
    collect(&h, &[active_info(&h, "1")]);
    assert_eq!(
        calls(&h).len(),
        2,
        "a failure must back off, not retry every pass"
    );
    h.sandbox.clock.advance(FAILURE_RETRY_S);
    ok(&h, &lapsed(), "o1", "");
    let plans = collect(&h, &[active_info(&h, "1")]);
    assert_eq!(plans["1"]["plan"], Value::Object(lapsed()));
    assert!(plans["1"].get("retryAt").is_none());
}

#[test]
fn retry_after_outlasts_the_default_backoff() {
    let h = harness();
    h.sandbox.http.queue(
        PROFILE_URL,
        Ok(crate::http::Response {
            status: 429,
            headers: vec![("retry-after".into(), "7200".into())],
            body: b"{}".to_vec(),
        }),
    );
    collect(&h, &[active_info(&h, "1")]);
    h.sandbox.clock.advance(FAILURE_RETRY_S + 1.);
    collect(&h, &[active_info(&h, "1")]);
    assert_eq!(calls(&h).len(), 1);
    h.sandbox.clock.advance(7200.);
    collect(&h, &[active_info(&h, "1")]);
    assert_eq!(calls(&h).len(), 2);
}

#[test]
fn a_response_for_another_organization_is_not_kept() {
    let h = harness();
    ok(&h, &lapsed(), "other", "");
    let plans = collect(&h, &[active_info(&h, "1")]);
    assert!(plans.is_empty());
    let record = &h.switcher.subscription_store().load()[&identity_key("a@x", Some("o1"))];
    assert!(record.get("plan").is_none() && record.get("retryAt").is_some());
}

#[test]
fn a_response_for_another_account_is_not_kept() {
    let h = harness();
    let mut mine = active_info(&h, "1");
    mine.slot.identity.uuid = "mine".into();
    ok(&h, &lapsed(), "o1", "theirs");
    let plans = collect(&h, &[mine]);
    assert!(plans.is_empty());
}

#[test]
fn the_fallback_read_uses_the_display_cache() {
    let h = harness();
    let account = backup_username("2", "b@x");
    h.set_backup("2", "b@x", &token_until("cached", future_ms(&h)));
    h.switcher.store.read_account_credentials("2", "b@x", true);
    h.sandbox.security.set(
        BACKUP_KEYCHAIN_SERVICE,
        &account,
        &token_until("fresh", future_ms(&h)),
    );
    ok(&h, &team(), "o1", "");
    collect(
        &h,
        &[info(
            "2",
            "b@x",
            "o1",
            false,
            &token_until("stale", past_ms(&h)),
        )],
    );
    assert_eq!(
        calls(&h),
        ["cached"],
        "the fallback read went past the display cache"
    );
}

#[test]
fn an_empty_fetch_set_reads_stored_plans_only() {
    let h = harness();
    ok(&h, &team(), "o1", "");
    collect(&h, &[active_info(&h, "1")]);
    h.sandbox.clock.advance(PLAN_TTL_S + 1.);
    let nothing = BTreeSet::new();
    let pass = Fetch::Only {
        only: &nothing,
        scheduled: false,
    };
    let plans = h
        .switcher
        .plans_for_pass(&[active_info(&h, "1")], pass)
        .unwrap();
    assert_eq!(plans["1"]["plan"], Value::Object(team()));
    assert_eq!(calls(&h).len(), 1, "network on an empty fetch set");
}

#[test]
fn a_restricted_fetch_set_still_refreshes() {
    let h = harness();
    ok(&h, &team(), "o1", "");
    let two = BTreeSet::from(["2".to_owned()]);
    let pass = Fetch::Only {
        only: &two,
        scheduled: false,
    };
    h.switcher
        .plans_for_pass(&[active_info(&h, "1")], pass)
        .unwrap();
    assert_eq!(calls(&h), ["live"]);
}

#[test]
fn inactive_slot_rereads_a_token_the_usage_pass_refreshed() {
    let h = harness();
    h.set_backup("2", "b@x", &token_until("refreshed", future_ms(&h)));
    ok(&h, &team(), "o1", "");
    collect(
        &h,
        &[info(
            "2",
            "b@x",
            "o1",
            false,
            &token_until("stale", past_ms(&h)),
        )],
    );
    assert_eq!(calls(&h), ["refreshed"]);
}

#[test]
fn no_valid_token_means_no_call_and_no_record() {
    let h = harness();
    h.set_backup("2", "b@x", &token_until("still-stale", past_ms(&h)));
    ok(&h, &team(), "o1", "");
    let plans = collect(
        &h,
        &[info(
            "2",
            "b@x",
            "o1",
            false,
            &token_until("stale", past_ms(&h)),
        )],
    );
    assert!(calls(&h).is_empty());
    assert!(!plans.contains_key("2"));
}

#[test]
fn active_slot_never_falls_back_to_the_slot_backup() {
    let h = harness();
    h.set_backup("1", "a@x", &token_until("backup", future_ms(&h)));
    ok(&h, &team(), "o1", "");
    h.sandbox.security.clear_calls();
    collect(
        &h,
        &[info(
            "1",
            "a@x",
            "o1",
            true,
            &token_until("live", past_ms(&h)),
        )],
    );
    assert!(
        h.sandbox.security.calls().is_empty(),
        "the slot's backup was read"
    );
    assert!(calls(&h).is_empty());
}

#[test]
fn a_slot_without_oauth_is_skipped() {
    let h = harness();
    h.set_backup("3", "k@x", "sk-ant-api");
    ok(&h, &team(), "o1", "");
    let plans = collect(&h, &[info("3", "k@x", "", false, "sk-ant-api")]);
    assert!(calls(&h).is_empty());
    assert!(plans.is_empty());
}

#[test]
fn the_record_follows_the_identity_when_slots_are_renumbered() {
    let h = harness();
    ok(&h, &lapsed(), "o1", "");
    collect(&h, &[active_info(&h, "1")]);
    let plans = collect(&h, &[active_info(&h, "4")]);
    assert_eq!(calls(&h).len(), 1, "refetched");
    assert_eq!(plans["4"]["plan"], Value::Object(lapsed()));
}

#[test]
fn lands_in_the_switchers_own_backup_dir() {
    let h = harness();
    ok(&h, &team(), "o1", "");
    collect(&h, &[active_info(&h, "1")]);
    assert!(h.sandbox.store().join("cache").join(PLAN_FILENAME).exists());
}

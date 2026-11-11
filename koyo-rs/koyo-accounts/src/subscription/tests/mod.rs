//! Tests translated from claude-swap's `tests/test_subscription.py` (0fe22a8), named after
//! cswap's: the fetch, label, records and the plan store (`TestIsExpired` is left out with
//! `is_expired`). The collector (`TestCollector`, the switcher's `_collect_subscription_plans`)
//! and the views come with the switcher and view ports. The plan store's tests are in
//! `store.rs`.

mod store;

use std::time::Duration;

use serde_json::{Map, Value, json};

use super::*;
use crate::http::{HttpError, Response};
use crate::testing::Sandbox;

pub(super) fn obj(value: Value) -> Map<String, Value> {
    let Value::Object(map) = value else {
        panic!("not an object: {value}")
    };
    map
}

pub(super) fn active_max() -> Map<String, Value> {
    obj(json!({
        "tier": "default_claude_max_20x", "billingType": "stripe_subscription",
        "subscriptionStatus": "active", "planDisplayName": null,
    }))
}

pub(super) fn lapsed() -> Map<String, Value> {
    obj(json!({
        "tier": "default_claude_ai", "billingType": "none",
        "subscriptionStatus": "canceled", "planDisplayName": null,
    }))
}

pub(super) fn team() -> Map<String, Value> {
    obj(json!({
        "tier": "default_claude_max_5x", "organizationType": "claude_team",
        "billingType": "stripe_subscription", "subscriptionStatus": "active",
        "planDisplayName": null,
    }))
}

pub(super) fn enterprise() -> Map<String, Value> {
    obj(json!({
        "tier": "default_claude_zero", "organizationType": "claude_enterprise",
        "billingType": "stripe_subscription_contracted", "subscriptionStatus": "active",
        "planDisplayName": null,
    }))
}

pub(super) fn unnamed() -> Map<String, Value> {
    obj(json!({"tier": "default_claude_max_5x"}))
}

pub(super) fn ultra() -> Map<String, Value> {
    obj(json!({
        "tier": "default_claude_max_20x", "organizationType": "claude_max",
        "planDisplayName": "Ultra",
    }))
}

fn creds(token: Option<&str>, expires_ms: Option<f64>) -> String {
    let mut blob = Map::new();
    if let Some(token) = token {
        blob.insert("accessToken".into(), json!(token));
    }
    if let Some(ms) = expires_ms {
        blob.insert("expiresAt".into(), json!(ms as i64));
    }
    json!({"claudeAiOauth": blob}).to_string()
}

fn fetched(response: Result<Response, HttpError>) -> PlanFetch {
    let sandbox = Sandbox::new();
    sandbox.http.queue(PROFILE_URL, response);
    fetch_plan(&sandbox.env(), "tok", PLAN_TIMEOUT)
}

fn ok_json(payload: &Value) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: payload.to_string().into_bytes(),
    }
}

fn status(code: u16, headers: &[(&str, &str)]) -> Response {
    Response {
        status: code,
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        body: Vec::new(),
    }
}

// TestFetchPlan

#[test]
fn keeps_the_four_plan_fields_in_camel_case() {
    let sandbox = Sandbox::new();
    sandbox.http.queue_json(
        PROFILE_URL,
        200,
        &json!({
            "account": {"uuid": "u", "email": "e@x"},
            "organization": {
                "rate_limit_tier": "default_claude_max_5x",
                "organization_type": "claude_team",
                "billing_type": "stripe_subscription",
                "subscription_status": "active",
                "name": "not kept",
                "uuid": "o",
            },
        }),
    );
    let result = fetch_plan(&sandbox.env(), "tok", PLAN_TIMEOUT);
    assert_eq!(result.plan, Some(team()));
    assert_eq!(
        (
            result.account_uuid.as_str(),
            result.organization_uuid.as_str()
        ),
        ("u", "o")
    );
    let request = &sandbox.http.requests()[0];
    assert_eq!(request.timeout, Duration::from_secs(5));
    assert!(
        request
            .headers
            .contains(&("Authorization".into(), "Bearer tok".into()))
    );
}

#[test]
fn keeps_the_plan_display_name() {
    let result = fetched(Ok(ok_json(&json!({"organization": {
        "rate_limit_tier": "default_claude_max_20x",
        "organization_type": "claude_max",
        "plan_display_name": "Ultra",
    }}))));
    assert_eq!(result.plan, Some(ultra()));
}

#[test]
fn no_tier_is_no_answer() {
    let result = fetched(Ok(ok_json(
        &json!({"organization": {"billing_type": "stripe_subscription"}}),
    )));
    assert_eq!(result.plan, None);
}

#[test]
fn empty_tier_is_no_answer() {
    let result = fetched(Ok(ok_json(
        &json!({"organization": {"rate_limit_tier": ""}}),
    )));
    assert_eq!(result.plan, None);
}

#[test]
fn non_string_fields_are_dropped_not_kept() {
    let result = fetched(Ok(ok_json(&json!({"organization": {
        "rate_limit_tier": "default_claude_ai",
        "subscription_status": null,
        "billing_type": 3,
        "plan_display_name": "",
    }}))));
    assert_eq!(
        result.plan,
        Some(obj(
            json!({"tier": "default_claude_ai", "planDisplayName": null})
        ))
    );
}

#[test]
fn any_transport_failure_is_none() {
    for failure in [
        Ok(status(401, &[])),
        Err(HttpError::Network("down".into())),
        Err(HttpError::Timeout),
    ] {
        assert_eq!(fetched(failure).plan, None);
    }
}

#[test]
fn retry_after_is_carried_out_of_a_429() {
    let result = fetched(Ok(status(429, &[("Retry-After", "3600")])));
    assert_eq!(result.plan, None);
    assert_eq!(result.retry_after_s, Some(3600.));
}

#[test]
fn malformed_body_is_none() {
    let result = fetched(Ok(Response {
        status: 200,
        headers: Vec::new(),
        body: b"not json".to_vec(),
    }));
    assert_eq!(result.plan, None);
}

#[test]
fn non_dict_organization_is_none() {
    assert_eq!(
        fetched(Ok(ok_json(&json!({"organization": []})))).plan,
        None
    );
}

// TestPlanLabel

#[test]
fn organization_wins_over_the_seat_tier() {
    for (plan, label) in [(team(), "Team"), (enterprise(), "Enterprise")] {
        assert_eq!(plan_label(Some(&plan)).as_deref(), Some(label));
        let mut named = plan.clone();
        named.insert("planDisplayName".into(), json!("Ultra"));
        assert_eq!(plan_label(Some(&named)).as_deref(), Some(label));
    }
}

#[test]
fn server_name_wins_unless_the_table_is_more_precise() {
    for (tier, name, label) in [
        (json!("default_claude_max_20x"), json!("Ultra"), "Ultra"),
        (json!("default_claude_max_20x"), json!(""), "Max 20x"),
        (json!("default_claude_max_20x"), Value::Null, "Max 20x"),
        (json!("default_claude_max_5x"), json!("Max"), "Max 5x"),
        (json!("default_claude_max_5x"), json!(""), "Max 5x"),
        (json!("default_claude_pro"), json!("Pro"), "Pro"),
        (Value::Null, json!("Ultra"), "Ultra"),
    ] {
        let mut plan = ultra();
        plan.insert("tier".into(), tier.clone());
        plan.insert("planDisplayName".into(), name.clone());
        assert_eq!(
            plan_label(Some(&plan)).as_deref(),
            Some(label),
            "{tier} {name}"
        );
    }
}

#[test]
fn pro_organization_wins_over_the_default_tier() {
    let plan = obj(json!({
        "tier": "default_claude_ai", "organizationType": "claude_pro",
        "billingType": "apple_subscription",
    }));
    assert_eq!(plan_label(Some(&plan)).as_deref(), Some("Pro"));
}

#[test]
fn observed_tiers() {
    for (tier, label) in [
        ("default_claude_ai", "Free"),
        ("default_claude_max_5x", "Max 5x"),
        ("default_claude_max_20x", "Max 20x"),
    ] {
        assert_eq!(
            plan_label(Some(&obj(json!({"tier": tier})))).as_deref(),
            Some(label)
        );
    }
}

#[test]
fn unobserved_tier_shows_raw_rather_than_a_guess() {
    let plan = obj(json!({"tier": "default_claude_pro"}));
    assert_eq!(
        plan_label(Some(&plan)).as_deref(),
        Some("default_claude_pro")
    );
}

#[test]
fn unknown_organization_type_falls_through_to_the_tier() {
    let plan = obj(json!({"tier": "default_claude_max_20x", "organizationType": "claude_max"}));
    assert_eq!(plan_label(Some(&plan)).as_deref(), Some("Max 20x"));
}

#[test]
fn no_plan_no_label() {
    assert_eq!(plan_label(None), None);
}

// TestRecords

fn record(plan: Map<String, Value>, fetched_at: Value) -> Value {
    Value::Object(Map::from_iter([
        ("plan".to_owned(), Value::Object(plan)),
        ("fetchedAt".to_owned(), fetched_at),
    ]))
}

#[test]
fn missing_is_due() {
    assert!(is_due(None, 1000., PLAN_TTL_S));
}

#[test]
fn fresh_is_due_only_when_the_name_key_is_missing() {
    for (plan, due) in [(team(), false), (unnamed(), true)] {
        let fresh = record(plan, json!(1000.0));
        assert_eq!(is_due(Some(&fresh), 1060., PLAN_TTL_S), due);
    }
}

#[test]
fn due_at_ttl() {
    let fresh = record(team(), json!(1000.0));
    assert!(is_due(Some(&fresh), 1000. + PLAN_TTL_S, PLAN_TTL_S));
}

#[test]
fn future_stamp_is_clock_skew_and_due() {
    let skewed = record(team(), json!(5000.0));
    assert!(is_due(Some(&skewed), 1000., PLAN_TTL_S));
}

#[test]
fn malformed_record_is_invalid_and_due() {
    for bad in [
        json!({"plan": team()}),
        json!({"fetchedAt": 1.0}),
        json!({"plan": {"label": "no tier"}, "fetchedAt": 1.0}),
        json!({"plan": team(), "fetchedAt": true}),
        json!("not a dict"),
    ] {
        assert!(!valid_record(&bad), "{bad}");
        assert!(is_due(Some(&bad), 1000., PLAN_TTL_S), "{bad}");
    }
}

#[test]
fn a_live_retry_holds_an_overdue_record() {
    for plan in [team(), unnamed()] {
        let held = json!({"plan": plan, "fetchedAt": 0.0, "retryAt": 2000.0});
        assert!(!is_due(Some(&held), 1000., PLAN_TTL_S));
    }
}

#[test]
fn a_passed_retry_does_not_hold_it() {
    let now = PLAN_TTL_S + 1000.;
    let held = json!({"plan": team(), "fetchedAt": 0.0, "retryAt": now - 1.});
    assert!(is_due(Some(&held), now, PLAN_TTL_S));
}

#[test]
fn an_impossibly_distant_retry_is_skew() {
    let now = PLAN_TTL_S + 1000.;
    let held = json!({"plan": team(), "fetchedAt": 0.0, "retryAt": 1e12});
    assert!(is_due(Some(&held), now, PLAN_TTL_S));
}

#[test]
fn identity_key_separates_organizations() {
    assert_ne!(
        identity_key("a@x", Some("org-1")),
        identity_key("a@x", Some("org-2"))
    );
    assert_eq!(identity_key("a@x", None), "a@x\n");
}

// TestMatchesSlot

#[test]
fn every_recorded_uuid_must_match() {
    let fetched = PlanFetch {
        plan: Some(team()),
        account_uuid: "a".into(),
        organization_uuid: "o".into(),
        retry_after_s: None,
    };
    for (uuid, org, expected) in [
        ("a", "o", true),
        ("a", "", true),
        ("", "o", true),
        ("x", "o", false),
        ("a", "x", false),
        ("", "", false),
    ] {
        assert_eq!(matches_slot(&fetched, uuid, org), expected, "{uuid} {org}");
    }
}

#[test]
fn a_recorded_uuid_the_response_lacks_does_not_match() {
    let fetched = PlanFetch {
        plan: Some(team()),
        organization_uuid: "o".into(),
        ..PlanFetch::default()
    };
    assert!(!matches_slot(&fetched, "a", "o"));
}

// TestUsableAccessToken

const NOW: f64 = 1_800_000_000.;

#[test]
fn valid_token() {
    let token = usable_access_token(Some(&creds(Some("t"), Some((NOW + 3600.) * 1000.))), NOW);
    assert_eq!(token.as_deref(), Some("t"));
}

#[test]
fn expired_token_is_not_usable() {
    let token = usable_access_token(Some(&creds(Some("t"), Some((NOW - 3600.) * 1000.))), NOW);
    assert_eq!(token, None);
}

#[test]
fn missing_token() {
    let token = usable_access_token(Some(&creds(None, Some((NOW + 3600.) * 1000.))), NOW);
    assert_eq!(token, None);
}

#[test]
fn garbage() {
    for garbage in [None, Some(""), Some("not json"), Some("{}")] {
        assert_eq!(usable_access_token(garbage, NOW), None, "{garbage:?}");
    }
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "whole seconds added to whole seconds are exact"
)]
fn a_zero_retry_after_still_gets_the_floor() {
    assert_eq!(next_retry_at(100., Some(0.)), 100. + FAILURE_RETRY_S);
    assert_eq!(next_retry_at(100., Some(7200.)), 7300.);
    assert_eq!(next_retry_at(100., Some(1e9)), 100. + MAX_RETRY_S);
}

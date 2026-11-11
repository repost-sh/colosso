//! From claude-swap's `tests/test_oauth_weekly_only.py` (0fe22a8): an account may report a weekly quota without a 5-hour
//! window (a verified Ultra account answers `five_hour: null`). An absent window is not a zero
//! reading, `utilization: null` is unknown rather than zero or absent, an unreadable window
//! fails the measurement, and a failed fetch is not headroom. The views' halves of these tests
//! (menu bar, TUI, CLI lines, JSON) and the engine's are left for the ports that bring them.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{answer, num};
use crate::http::HttpError;
use crate::oauth::*;
use crate::testing::Sandbox;
use crate::usage_store::{Decision, FetchRecord, Identities, UsageStore};

const CREDENTIALS: &str = r#"{"claudeAiOauth": {"accessToken": "test-access",
    "refreshToken": "test-refresh", "expiresAt": 4102444800000,
    "scopes": ["user:profile", "user:inference", "user:sessions:claude_code"]}}"#;

/// `try_fetch_usage_for_account` for account 2 against a canned body.
fn fetch_body(sandbox: &Sandbox, body: &str) -> UsageOutcome {
    let before = sandbox.http.requests().len();
    sandbox.http.queue(USAGE_URL, Ok(answer(200, &[], body)));
    let outcome = try_fetch_usage_for_account(&sandbox.env(), "2", CREDENTIALS);
    let requests = sandbox.http.requests();
    assert_eq!(requests.len(), before + 1);
    assert_eq!(
        requests.last().unwrap().url,
        "https://api.anthropic.com/api/oauth/usage"
    );
    outcome
}

fn fetch(sandbox: &Sandbox, payload: &Value) -> UsageOutcome {
    fetch_body(sandbox, &payload.to_string())
}

fn identities() -> Identities {
    BTreeMap::from([(
        "2".to_owned(),
        ("weekly@example.com".to_owned(), String::new()),
    )])
}

fn record(store: &UsageStore, outcome: &UsageOutcome) {
    let record = FetchRecord {
        usage: outcome.usage.clone(),
        error: outcome.error.clone(),
        ..FetchRecord::default()
    };
    store
        .record(
            &BTreeMap::from([("2".to_owned(), record)]),
            &identities(),
            None,
            None,
        )
        .unwrap();
}

#[test]
fn an_absent_five_hour_window_is_a_limit_the_account_does_not_have() {
    for (five_hour, present) in [
        (Some(json!({"utilization": 10.0})), true),
        (Some(Value::Null), false),
        (None, false),
        // `{}` stays absent: the falsy check this replaces skipped it.
        (Some(json!({})), false),
    ] {
        let sandbox = Sandbox::new();
        let mut payload = json!({"seven_day": {"utilization": 25.0}});
        if let Some(window) = five_hour {
            payload["five_hour"] = window;
        }
        let outcome = fetch(&sandbox, &payload);
        assert_eq!(outcome.error, None);
        let usage = outcome.usage.unwrap();
        assert_eq!(num(&usage, &["seven_day", "pct"]), Some(25.));
        assert_eq!(usage.get("five_hour").is_some(), present);
        assert_eq!(account_headroom(Some(&usage), &[]), Some(75.));
    }
}

#[test]
fn an_account_without_a_reading_has_no_usage() {
    let sandbox = Sandbox::new();
    for payload in [json!({}), json!({"five_hour": null, "seven_day": null})] {
        let outcome = fetch(&sandbox, &payload);
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.usage, None);
        assert_eq!(account_headroom(outcome.usage.as_ref(), &[]), None);
    }
    sandbox.http.queue(USAGE_URL, Err(HttpError::Timeout));
    let outcome = try_fetch_usage_for_account(&sandbox.env(), "2", CREDENTIALS);
    assert_eq!(outcome.error.as_deref(), Some("timeout"));
    assert_eq!(outcome.usage, None);
}

#[test]
fn an_unreadable_window_fails_the_whole_reading() {
    // Every other falsy value is a malformed window, not a missing one. NaN, infinity and an
    // overflowing integer are what `json.dumps` writes for them, which a JSON reader refuses.
    let broken = [
        "false",
        "0",
        r#""""#,
        "[]",
        r#"["0"]"#,
        "true",
        r#""50""#,
        r#"{"resets_at": null}"#,
        r#"{"utilization": "0"}"#,
        r#"{"utilization": false}"#,
        r#"{"utilization": NaN}"#,
        r#"{"utilization": Infinity}"#,
        &format!(r#"{{"utilization": 1{}}}"#, "0".repeat(400)),
        r#"{"utilization": -1}"#,
    ];
    for window in ["five_hour", "seven_day"] {
        for bad in &broken {
            let sandbox = Sandbox::new();
            let mut parts = vec![
                (
                    String::from("five_hour"),
                    r#"{"utilization": 10.0}"#.to_owned(),
                ),
                (
                    String::from("seven_day"),
                    r#"{"utilization": 10.0}"#.to_owned(),
                ),
            ];
            for part in &mut parts {
                if part.0 == window {
                    part.1 = (*bad).to_owned();
                }
            }
            let body = format!(
                "{{{}}}",
                parts
                    .iter()
                    .map(|(key, value)| format!(r#""{key}": {value}"#))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let outcome = fetch_body(&sandbox, &body);
            assert_eq!(
                outcome.error.as_deref(),
                Some("bad-response"),
                "{window}: {bad}"
            );
            assert_eq!(outcome.usage, None);
        }
    }
}

#[test]
fn an_unreadable_window_keeps_the_last_good_reading() {
    let sandbox = Sandbox::new();
    sandbox.clock.set(10_000_000.);
    let store = UsageStore::new(&sandbox.env());
    let good = fetch(&sandbox, &json!({"seven_day": {"utilization": 83.0}}));
    record(&store, &good);
    let fetched_at = sandbox.env().now();

    sandbox.clock.advance(60.);
    let bad = fetch(
        &sandbox,
        &json!({"five_hour": {"utilization": "0"}, "seven_day": {"utilization": 1.0}}),
    );
    record(&store, &bad);
    let entry = store.entries(&identities(), &[]).remove("2").unwrap();
    assert_eq!(entry.last_error.as_deref(), Some("bad-response"));
    assert_eq!(entry.last_good, good.usage);
    assert_eq!(entry.fetched_at, Some(fetched_at));
    // The last good reading survives; the unreadable 1% never becomes headroom.
    assert_eq!(
        entry.decision_value(),
        good.usage.as_ref().map(Decision::Usage)
    );
}

#[test]
fn a_window_without_a_utilization_figure_is_unknown_not_absent() {
    for (window, other, label) in [
        ("five_hour", "seven_day", "7d"),
        ("seven_day", "five_hour", "5h"),
    ] {
        for resets_at in [None, Some("2099-01-01T05:00:00+00:00")] {
            let sandbox = Sandbox::new();
            let mut reported = json!({"utilization": null});
            if let Some(at) = resets_at {
                reported["resets_at"] = json!(at);
            }
            let mut payload = json!({other: {"utilization": 10.0}});
            payload[window] = reported;
            let outcome = fetch(&sandbox, &payload);
            assert_eq!(outcome.error, None);
            let usage = outcome.usage.unwrap();
            let entry = &usage[window];
            assert!(entry["pct"].is_null());
            assert_eq!(entry.get("resets_at").and_then(Value::as_str), resets_at);
            assert_eq!(
                entry.get("countdown").is_some() && entry.get("clock").is_some(),
                resets_at.is_some()
            );
            assert_eq!(usage[other], json!({"pct": 10.0}));
            // Decisions use only the window that carries a figure.
            assert_eq!(account_headroom(Some(&usage), &[]), Some(90.));
            let labels: Vec<String> = relevant_windows(Some(&usage), &[])
                .into_iter()
                .map(|w| w.label)
                .collect();
            assert_eq!(labels, [label]);
        }
    }
}

#[test]
fn a_reading_with_an_unknown_window_replaces_the_last_good_one() {
    let sandbox = Sandbox::new();
    sandbox.clock.set(10_000_000.);
    let store = UsageStore::new(&sandbox.env());
    let old = fetch(&sandbox, &json!({"seven_day": {"utilization": 83.0}}));
    record(&store, &old);

    sandbox.clock.advance(60.);
    let new = fetch(
        &sandbox,
        &json!({
            "five_hour": {"utilization": null, "resets_at": "2099-01-01T05:00:00+00:00"},
            "seven_day": {"utilization": 1.0},
        }),
    );
    assert_eq!(new.error, None);
    record(&store, &new);
    let entry = store.entries(&identities(), &[]).remove("2").unwrap();
    assert_eq!(entry.last_error, None);
    assert_eq!(entry.fetched_at, Some(sandbox.env().now()));
    assert_eq!(
        entry.decision_value(),
        new.usage.as_ref().map(Decision::Usage)
    );
    let value = entry.decision_value().and_then(Decision::usage);
    assert_eq!(account_headroom(value, &[]), Some(99.));
}

#[test]
fn an_unknown_window_alone_is_no_figure_to_rank_on() {
    let sandbox = Sandbox::new();
    let outcome = fetch(
        &sandbox,
        &json!({"five_hour": {"utilization": null}, "seven_day": null}),
    );
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.usage, Some(json!({"five_hour": {"pct": null}})));
    assert_eq!(account_headroom(outcome.usage.as_ref(), &[]), None);
}

#[test]
fn a_new_weekly_only_reading_drops_the_previous_five_hour_window() {
    let sandbox = Sandbox::new();
    let store = UsageStore::new(&sandbox.env());
    let old = fetch(
        &sandbox,
        &json!({"five_hour": {"utilization": 100.0}, "seven_day": {"utilization": 25.0}}),
    );
    record(&store, &old);
    let current = fetch(
        &sandbox,
        &json!({"five_hour": null, "seven_day": {"utilization": 25.0}}),
    );
    record(&store, &current);
    let entries = store.entries(&identities(), &[]);
    let value = entries["2"].decision_value().and_then(Decision::usage);
    assert_eq!(value, Some(&json!({"seven_day": {"pct": 25.0}})));
}

#[test]
fn an_over_limit_reading_is_retained_without_clamping() {
    let sandbox = Sandbox::new();
    let outcome = fetch(&sandbox, &json!({"seven_day": {"utilization": 105.0}}));
    assert_eq!(outcome.error, None);
    let usage = outcome.usage.unwrap();
    assert_eq!(num(&usage, &["seven_day", "pct"]), Some(105.));
    assert_eq!(account_headroom(Some(&usage), &[]), Some(-5.));
}

/// The store's half of cswap's `test_a_rejected_probe_with_an_unreadable_window_keeps_its_refusal`:
/// a reading carrying a refusal with a `Retry-After` is stored as a success that blocks the
/// account, backing off until the refusal ends.
#[test]
fn a_rejected_reading_is_stored_at_its_limit() {
    let sandbox = Sandbox::new();
    let now = sandbox.env().now();
    let store = UsageStore::new(&sandbox.env());
    let rejected =
        json!({"rejected": {"retry_after_s": 600.0, "resets_at": super::iso(now + 600.)}});
    record(&store, &UsageOutcome::read(Some(rejected.clone())));
    let entry = store.entries(&identities(), &[]).remove("2").unwrap();
    assert_eq!(entry.last_error, None);
    assert_eq!(entry.last_good, Some(rejected));
    let value = entry.decision_value().and_then(Decision::usage);
    assert_eq!(account_headroom(value, &[]), Some(0.));
    assert_eq!(entry.backoff_until, Some(now + 600.));
}

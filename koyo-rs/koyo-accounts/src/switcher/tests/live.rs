//! Koyo's own addition, `Switcher::adopt_live_limits` (not in claude-swap, so no cswap test):
//! a chat reply's windows laid over the active slot's stored reading, as the spec asks
//! (`docs/claude-accounts.md` 4): the merge, no erase, a newer stored reading kept, nothing for
//! an unmanaged login, and the reading cswap's header parser would fail.

use serde_json::{Value, json};

use super::Harness;
use super::support::{EMAIL, gate_harness, recorded};
use crate::Clock as _;
use crate::switcher::{LiveLimits, LiveWindow};
use crate::testing::entry_as;
use crate::usage_store::{UsageEntry, one_identity};

/// Slot 1 live with a stored reading taken `age_s` ago (negative: ahead of the clock).
fn stored(age_s: f64) -> Harness {
    let h = gate_harness();
    h.mock_claude_config();
    let reading = json!({
        "five_hour": {"pct": 10.0},
        "seven_day": {"pct": 20.0},
        "spend": {"used": 1.5, "limit": 50.0, "pct": 3.0, "currency": "USD"},
        "scoped": [{"name": "Fable", "pct": 30.0}],
    });
    recorded(&h, "1", reading, &one_identity("1", EMAIL, ""), age_s);
    h
}

fn entry(h: &Harness) -> UsageEntry {
    entry_as(&h.switcher.usage_store, "1", &one_identity("1", EMAIL, ""))
}

fn last_good(h: &Harness) -> Value {
    entry(h).last_good.unwrap()
}

fn window(h: &Harness, fraction: f64, ahead_s: f64) -> LiveWindow {
    LiveWindow {
        utilization_fraction: fraction,
        resets_at: Some(h.sandbox.clock.now() + ahead_s),
    }
}

fn adopted(h: &Harness, limits: LiveLimits) -> bool {
    h.switcher.adopt_live_limits(limits).unwrap().is_some()
}

#[test]
fn a_reply_s_windows_are_laid_over_the_stored_reading() {
    let h = stored(600.);
    let limits = LiveLimits {
        five_hour: Some(window(&h, 0.4237, 3600.)),
        seven_day: Some(window(&h, 0.5, 86_400.)),
        seven_day_overage_included: Some(window(&h, 0.61, 86_400.)),
    };
    let returned = h.switcher.adopt_live_limits(limits).unwrap();
    assert_eq!(
        returned,
        Some(entry(&h)),
        "the entry as adopted is the stored one"
    );
    let reading = last_good(&h);
    assert_eq!(reading["five_hour"]["pct"], 42.37);
    // Python's `isoformat()` of the epoch reset, as cswap's header parser writes it.
    let reset = chrono::DateTime::from_timestamp(h.sandbox.clock.now() as i64 + 3600, 0).unwrap();
    assert_eq!(
        reading["five_hour"]["resets_at"],
        reset.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    );
    assert!(reading["five_hour"]["countdown"].is_string());
    assert_eq!(reading["seven_day"]["pct"], 50.0);
    assert_eq!(
        reading["scoped"].as_array().unwrap().len(),
        1,
        "Fable appended, not replaced"
    );
    assert_eq!(reading["scoped"][0]["name"], "Fable");
    assert_eq!(reading["scoped"][0]["pct"], 61.0);
    assert_eq!(
        reading["spend"]["used"], 1.5,
        "the stored spend line is kept"
    );
    assert_eq!(entry(&h).age_s, Some(0.), "the reply's reading is of now");
}

#[test]
fn a_window_the_reply_did_not_carry_keeps_its_stored_value() {
    let h = stored(600.);
    let limits = LiveLimits {
        five_hour: Some(window(&h, 0.9, 3600.)),
        ..LiveLimits::default()
    };
    assert!(adopted(&h, limits));
    let reading = last_good(&h);
    assert_eq!(reading["five_hour"]["pct"], 90.0);
    assert_eq!(reading["seven_day"]["pct"], 20.0);
    assert_eq!(
        reading["scoped"],
        json!([{"name": "Fable", "pct": 30.0}]),
        "Fable erased"
    );
}

#[test]
fn a_newer_stored_reading_is_kept() {
    let h = stored(-5.);
    let limits = LiveLimits {
        five_hour: Some(window(&h, 0.9, 3600.)),
        ..LiveLimits::default()
    };
    assert!(!adopted(&h, limits));
    assert_eq!(last_good(&h)["five_hour"]["pct"], 10.0);
}

#[test]
fn nothing_is_adopted_for_an_unmanaged_login() {
    let h = gate_harness();
    h.write_config(&json!({"oauthAccount": {"emailAddress": "stranger@example.com"}}));
    let limits = LiveLimits {
        five_hour: Some(window(&h, 0.5, 3600.)),
        ..LiveLimits::default()
    };
    assert!(!adopted(&h, limits));
    assert!(
        !h.switcher.usage_store.path.exists(),
        "a reading was written"
    );
}

#[test]
fn an_unusable_window_fails_the_reading_as_cswap_s_parser() {
    let h = stored(600.);
    let limits = LiveLimits {
        five_hour: Some(window(&h, f64::NAN, 3600.)),
        seven_day: Some(window(&h, 0.5, 86_400.)),
        ..LiveLimits::default()
    };
    assert!(!adopted(&h, limits));
    assert_eq!(last_good(&h)["seven_day"]["pct"], 20.0);
    let only_fable = LiveLimits {
        five_hour: Some(window(&h, 0.25, 3600.)),
        seven_day_overage_included: Some(window(&h, -1., 3600.)),
        ..LiveLimits::default()
    };
    assert!(adopted(&h, only_fable), "only the bucket is dropped");
    assert_eq!(last_good(&h)["scoped"][0]["pct"], 30.0);
}

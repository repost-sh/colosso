//! From claude-swap's `tests/test_oauth.py` (0fe22a8):
//! `TestAccountHeadroom`, `TestRelevantWindows`, `TestFormatReset`, and the
//! `TestHeaderReadingDecisions` cases that hold without the probe.

use serde_json::{Value, json};

use super::fixed_now;
use crate::oauth::*;
use crate::poll_policy::limiting_reset_ts;
use crate::testing::iso;

fn headroom(usage: &Value) -> Option<f64> {
    account_headroom(Some(usage), &[])
}

// TestAccountHeadroom

#[test]
fn binding_window_is_the_higher_utilization() {
    let usage = json!({"five_hour": {"pct": 80.0}, "seven_day": {"pct": 20.0}});
    assert_eq!(headroom(&usage), Some(20.));
}

#[test]
fn seven_day_can_be_the_binding_window() {
    let usage = json!({"five_hour": {"pct": 10.0}, "seven_day": {"pct": 95.0}});
    assert_eq!(headroom(&usage), Some(5.));
}

#[test]
fn single_window() {
    assert_eq!(headroom(&json!({"five_hour": {"pct": 40.0}})), Some(60.));
}

#[test]
fn at_limit_is_zero_headroom() {
    assert_eq!(headroom(&json!({"five_hour": {"pct": 100.0}})), Some(0.));
}

#[test]
fn spend_is_ignored() {
    // Pay-as-you-go credits must not drive rate-limit headroom.
    let usage = json!({"spend": {"pct": 99.0}, "five_hour": {"pct": 10.0}});
    assert_eq!(headroom(&usage), Some(90.));
}

#[test]
fn no_window_data_is_unknown() {
    assert_eq!(headroom(&json!({"spend": {"pct": 50.0}})), None);
    assert_eq!(headroom(&json!({})), None);
}

#[test]
fn none_and_non_dict_are_unknown() {
    assert_eq!(account_headroom(None, &[]), None);
    assert_eq!(headroom(&json!("no credentials")), None);
}

#[test]
fn malformed_pct_is_ignored() {
    assert_eq!(headroom(&json!({"five_hour": {"pct": null}})), None);
}

#[test]
fn scoped_ignored_without_models_arg() {
    let usage = json!({"five_hour": {"pct": 10.0}, "scoped": [{"name": "Fable", "pct": 100.0}]});
    assert_eq!(headroom(&usage), Some(90.));
}

#[test]
fn named_model_folds_into_binding_window() {
    let usage = json!({"five_hour": {"pct": 10.0}, "scoped": [{"name": "Fable", "pct": 95.0}]});
    assert_eq!(account_headroom(Some(&usage), &["Fable"]), Some(5.));
}

#[test]
fn maxed_model_is_at_limit_despite_session_headroom() {
    let usage = json!({
        "five_hour": {"pct": 1.0},
        "seven_day": {"pct": 40.0},
        "scoped": [{"name": "Fable", "pct": 100.0}],
    });
    assert_eq!(account_headroom(Some(&usage), &["Fable"]), Some(0.));
}

#[test]
fn model_match_is_case_insensitive() {
    let usage = json!({"scoped": [{"name": "Fable", "pct": 70.0}]});
    assert_eq!(account_headroom(Some(&usage), &["fable"]), Some(30.));
}

#[test]
fn unlisted_model_does_not_bind() {
    let usage = json!({"five_hour": {"pct": 10.0}, "scoped": [{"name": "Opus", "pct": 100.0}]});
    assert_eq!(account_headroom(Some(&usage), &["Fable"]), Some(90.));
}

#[test]
fn multiple_models_take_the_worst() {
    let usage = json!({
        "five_hour": {"pct": 10.0},
        "scoped": [
            {"name": "Fable", "pct": 30.0},
            {"name": "Opus", "pct": 95.0},
            {"name": "Haiku", "pct": 50.0},
        ],
    });
    // Opus binds (95%); Sonnet is absent and contributes nothing.
    assert_eq!(
        account_headroom(Some(&usage), &["Fable", "Opus", "Sonnet"]),
        Some(5.)
    );
}

#[test]
fn works_for_any_model_name() {
    for name in ["Opus", "Sonnet", "Haiku"] {
        let usage = json!({"scoped": [{"name": name, "pct": 100.0}]});
        assert_eq!(account_headroom(Some(&usage), &[name]), Some(0.));
    }
}

#[test]
fn only_scoped_and_named_yields_headroom() {
    let usage = json!({"scoped": [{"name": "Fable", "pct": 100.0}]});
    assert_eq!(account_headroom(Some(&usage), &["Fable"]), Some(0.));
}

#[test]
fn scoped_without_5h7d_and_unlisted_model_is_unknown() {
    let usage = json!({"scoped": [{"name": "Opus", "pct": 100.0}]});
    assert_eq!(account_headroom(Some(&usage), &["Fable"]), None);
}

#[test]
fn all_sentinel_matches_every_scoped_window() {
    let usage = json!({
        "five_hour": {"pct": 10.0},
        "scoped": [{"name": "Fable", "pct": 30.0}, {"name": "Sonnet", "pct": 97.0}],
    });
    assert_eq!(account_headroom(Some(&usage), &["all"]), Some(3.));
    assert_eq!(account_headroom(Some(&usage), &["ALL"]), Some(3.));
}

// TestRelevantWindows

#[test]
fn carries_labels_pcts_and_resets() {
    let usage = json!({
        "five_hour": {"pct": 80.0, "resets_at": "2026-07-10T12:00:00Z"},
        "seven_day": {"pct": 20.0},
        "scoped": [{"name": "Fable", "pct": 95.0, "resets_at": "2026-07-12T09:00:00Z"}],
    });
    assert_eq!(
        relevant_windows(Some(&usage), &["Fable"]),
        [
            Window::new("5h", 80., Some("2026-07-10T12:00:00Z")),
            Window::new("7d", 20., None),
            Window::new("Fable", 95., Some("2026-07-12T09:00:00Z")),
        ]
    );
}

#[test]
fn scoped_excluded_without_models() {
    let usage = json!({"five_hour": {"pct": 10.0}, "scoped": [{"name": "Fable", "pct": 99.0}]});
    assert_eq!(
        relevant_windows(Some(&usage), &[]),
        [Window::new("5h", 10., None)]
    );
}

#[test]
fn non_dict_usage_is_empty() {
    assert!(relevant_windows(None, &[]).is_empty());
    assert!(relevant_windows(Some(&json!("no credentials")), &[]).is_empty());
}

// TestFormatReset

#[test]
fn same_day_shows_time_only() {
    let (countdown, clock) =
        format_reset(&iso(fixed_now() + 2. * 3600. + 15. * 60.), fixed_now()).unwrap();
    assert_eq!(countdown, "2h 15m");
    assert_eq!(clock.matches(':').count(), 1);
}

#[test]
fn different_day_shows_date() {
    let (_, clock) = format_reset(&iso(fixed_now() + 2. * 86_400.), fixed_now()).unwrap();
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    assert!(months.iter().any(|month| clock.contains(month)), "{clock}");
}

#[test]
fn minutes_only_when_under_one_hour() {
    let (countdown, _) = format_reset(&iso(fixed_now() + 45. * 60.), fixed_now()).unwrap();
    assert_eq!(countdown, "45m");
    assert!(!countdown.contains('h'));
}

#[test]
fn reset_strings_read_what_cswap_reads() {
    let now = fixed_now();
    assert_eq!(format_reset("not a date", now), Err(ResetError::Value));
    // A time without an offset can't be subtracted from an aware one in cswap.
    assert_eq!(
        format_reset("2026-03-23T13:00:00", now),
        Err(ResetError::Type)
    );
    assert!(format_reset("2026-03-23T13:00:00Z", now).is_ok());
    assert!(format_reset("2026-03-23T13:00:00.123456+02:00", now).is_ok());
    assert_eq!(format_countdown(-5.), "0m");
    assert_eq!(format_countdown(5. * 86_400. + 12. * 3600. + 59.), "5d 12h");
    assert_eq!(format_countdown(3599.), "59m");
    // Drawn from `resets_at` when it reads, else the strings stored at fetch time.
    let stored = json!({"resets_at": "garbage", "countdown": "1h 0m", "clock": "13:00"});
    assert_eq!(
        fresh_reset_strings(stored.as_object().unwrap(), now),
        Some(("1h 0m".to_owned(), "13:00".to_owned()))
    );
    let fresh = json!({"resets_at": iso(now + 600.), "countdown": "stale", "clock": "x"});
    assert_eq!(
        fresh_reset_strings(fresh.as_object().unwrap(), now)
            .unwrap()
            .0,
        "10m"
    );
    assert_eq!(
        fresh_reset_strings(json!({}).as_object().unwrap(), now),
        None
    );
}

// TestHeaderReadingDecisions

#[test]
fn endpoint_reading_is_unchanged() {
    assert_eq!(headroom(&json!({"five_hour": {"pct": 5.0}})), Some(95.));
}

/// cswap's `test_rejection_binds_even_on_a_partial_reading`, without the probe's
/// completeness rule that hides the 5h window: the refusal still binds, and the reset
/// schedule waits for it to end.
#[test]
fn rejection_binds_even_on_a_partial_reading() {
    let resets_at = "2030-01-01T00:00:00+00:00";
    let usage = json!({
        "five_hour": {"pct": 5.0},
        "rejected": {"retry_after_s": 60.0, "resets_at": resets_at},
    });
    assert!(relevant_windows(Some(&usage), &[]).contains(&Window::new(
        REJECTED_LABEL,
        100.,
        Some(resets_at)
    )));
    assert_eq!(account_headroom(Some(&usage), &["Fable"]), Some(0.));
    assert_eq!(
        limiting_reset_ts(Some(&usage), &[]),
        Some(super::utc(2030, 1, 1, 0, 0, 0))
    );
}

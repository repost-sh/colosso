//! An account's usage as the accounts crate reads it (claude-swap's `build_usage_result` shape:
//! `seven_day` and `scoped` windows, each `{pct, resets_at}`) made into the panel's bars, with
//! claude-swap's pace marker on the week; what kept it from being read, an error kind or a
//! sentinel of the crate's, made into a problem the panel names with its remedy; and a chat's
//! rate-limit headers taken as the week's and the model's live numbers.

use koyo_accounts::oauth::{
    self, NETWORK_ERROR, ORG_POLICY_ERROR, RATE_LIMITED_ERROR, TIMEOUT_ERROR, TLS_CERT_ERROR,
    TOKEN_EXPIRED_ERROR,
};
use koyo_accounts::usage_store::PERMANENT_AUTH_ERRORS;
use koyo_accounts::{pace, poll_policy};
use koyo_agent::protocol::limits::{AccountLimits, LimitWindow};
use serde_json::Value;

use super::model::{Bar, Problem, Row, WEEKLY};

/// The per-model bucket the rate-limit headers carry, as claude-swap names it
/// (`oauth._UNIFIED_SCOPED_WINDOWS`: `7d_oi` is Fable's).
const HEADERS_MODEL: &str = "Fable";

fn bar(label: &str, window: &Value, measured_at: f64) -> Bar {
    Bar {
        label: label.to_owned().into(),
        percent: window.get("pct").and_then(Value::as_f64),
        resets_at: poll_policy::parse_reset_ts(window.get("resets_at").and_then(Value::as_str)),
        ahead_of_pace: pace::compute_pace(Some(window), Some(measured_at))
            .is_some_and(|pace| pace.ahead),
    }
}

/// The 7 days' bar, then each model's, from a reading measured at `measured_at`.
pub fn bars(usage: &Value, measured_at: f64) -> Vec<Bar> {
    let weekly = usage
        .get("seven_day")
        .map(|window| bar(WEEKLY, window, measured_at));
    let models = (usage.get("scoped").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|window| {
            let name = window.get("name").and_then(Value::as_str)?;
            Some(bar(name, window, measured_at))
        });
    weekly.into_iter().chain(models).collect()
}

/// What `kind`, an error kind or a sentinel of the accounts crate's, keeps from the reading;
/// `until`, when the store lets it be asked again.
pub fn problem(kind: &str, until: f64) -> Problem {
    match kind {
        RATE_LIMITED_ERROR => Problem::RateLimited { until },
        ORG_POLICY_ERROR => Problem::PlanLapsed,
        TLS_CERT_ERROR => Problem::CertificateRefused,
        TOKEN_EXPIRED_ERROR | oauth::USAGE_TOKEN_EXPIRED => Problem::TokenExpired,
        NETWORK_ERROR | TIMEOUT_ERROR => Problem::Offline,
        oauth::USAGE_RELOGIN_REQUIRED | oauth::USAGE_NO_CREDENTIALS => Problem::SignInAgain,
        oauth::USAGE_KEYCHAIN_UNAVAILABLE => Problem::KeychainUnavailable,
        kind if PERMANENT_AUTH_ERRORS.contains(&kind) => Problem::SignInAgain,
        other => Problem::Other(other.to_owned()),
    }
}

/// `window`, a fraction of 0–1 in the headers, as a bar's percent, to two places as claude-swap
/// rounds a header's.
fn live(label: &str, window: &LimitWindow, kept: Option<&Bar>) -> Bar {
    Bar {
        label: label.to_owned().into(),
        percent: Some((window.utilization * 10_000.).round() / 100.),
        resets_at: poll_policy::parse_reset_ts(Some(&window.resets_at.0)),
        ahead_of_pace: kept.is_some_and(|bar| bar.ahead_of_pace),
    }
}

/// Takes `limits` into `row` at `now`: its week and its model's window, those the headers
/// carried. Whether a bar changed.
pub fn take_limits(row: &mut Row, limits: &AccountLimits, now: f64) -> bool {
    let windows = [
        (WEEKLY, limits.seven_day.as_ref()),
        (HEADERS_MODEL, limits.seven_day_overage_included.as_ref()),
    ];
    let mut changed = false;
    for (label, window) in windows {
        let Some(window) = window else {
            continue;
        };
        if let Some(kept) = row.bars.iter_mut().find(|bar| bar.label == label) {
            let bar = live(label, window, Some(kept));
            changed |= *kept != bar;
            *kept = bar;
        } else {
            let at = if label == WEEKLY { 0 } else { row.bars.len() };
            row.bars.insert(at, live(label, window, None));
            changed = true;
        }
    }
    if limits.seven_day.is_some() || limits.seven_day_overage_included.is_some() {
        row.measured_at = Some(now);
        changed |= row.problem.take().is_some();
    }
    changed
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_week_comes_first_then_each_models_window() {
        let usage = json!({
            "five_hour": {"pct": 12.0},
            "seven_day": {"pct": 41.5, "resets_at": "2027-01-15T08:00:00+00:00"},
            "scoped": [{"name": "Fable", "pct": 70.0, "resets_at": "2027-01-15T08:00:00Z"}],
        });
        let bars = bars(&usage, 1_800_000_000.);
        let labels: Vec<&str> = bars.iter().map(|bar| bar.label.as_ref()).collect();
        assert_eq!(labels, ["7 days", "Fable"]);
        assert_eq!(bars[0].percent, Some(41.5));
        assert_eq!(bars[1].resets_at, bars[0].resets_at);
        assert!(bars[0].resets_at.is_some());
    }

    #[test]
    fn an_unknown_utilization_stays_unknown_and_no_week_is_no_bar() {
        let unknown = bars(&json!({"seven_day": {"pct": null}}), 1_800_000_000.);
        assert_eq!(unknown[0].percent, None);
        assert!(bars(&json!({"five_hour": {"pct": 3.0}}), 1_800_000_000.).is_empty());
    }

    #[test]
    fn kinds_are_named_by_what_fixes_them() {
        assert_eq!(
            problem(RATE_LIMITED_ERROR, 5.),
            Problem::RateLimited { until: 5. }
        );
        assert_eq!(problem(ORG_POLICY_ERROR, 0.), Problem::PlanLapsed);
        assert_eq!(problem(TLS_CERT_ERROR, 0.), Problem::CertificateRefused);
        assert_eq!(
            problem(oauth::USAGE_KEYCHAIN_UNAVAILABLE, 0.),
            Problem::KeychainUnavailable
        );
        assert_eq!(problem(TOKEN_EXPIRED_ERROR, 0.), Problem::TokenExpired);
        assert_eq!(problem("invalid_grant", 0.), Problem::SignInAgain);
        assert_eq!(problem("http-500", 0.), Problem::Other("http-500".into()));
    }
}

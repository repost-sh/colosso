//! An account of the accounts crate's snapshot made into the panel's row: its usage (claude-swap's
//! `build_usage_result` shape, `seven_day` and `scoped` windows, each `{pct, resets_at}`) as
//! bars with claude-swap's pace marker on the week, and what kept it from being read, an error
//! kind or a sentinel of the crate's, as a problem the panel names with its remedy; and a chat's
//! rate-limit headers as the crate's live limits.

use koyo_accounts::oauth::{
    self, NETWORK_ERROR, ORG_POLICY_ERROR, RATE_LIMITED_ERROR, TIMEOUT_ERROR, TLS_CERT_ERROR,
    TOKEN_EXPIRED_ERROR,
};
use koyo_accounts::switcher::{AccountSnapshot, LiveLimits, LiveWindow};
use koyo_accounts::usage_store::{PERMANENT_AUTH_ERRORS, UsageEntry};
use koyo_accounts::{pace, poll_policy};
use koyo_agent::protocol::limits::{AccountLimits, LimitWindow};
use serde_json::Value;

use super::model::{Bar, Problem, Row, WEEKLY};

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
        oauth::USAGE_FOREIGN_CREDENTIAL => Problem::ForeignLogin,
        kind if PERMANENT_AUTH_ERRORS.contains(&kind) => Problem::SignInAgain,
        other => Problem::Other(other.to_owned()),
    }
}

/// What `usage` keeps from a reading at `now`: its sentinel, or its last error while that keeps
/// it from being asked again or there's no reading at all.
fn entry_problem(usage: &UsageEntry, now: f64) -> Option<Problem> {
    if let Some(sentinel) = &usage.sentinel {
        return Some(problem(sentinel, 0.));
    }
    let kind = usage.last_error.as_deref()?;
    (usage.in_backoff(now) || usage.last_good.is_none())
        .then(|| problem(kind, usage.backoff_until.unwrap_or(0.)))
}

/// Takes `usage`, an account's entry, into `row` at `now`: its bars, when they were measured,
/// and what keeps them from being read.
pub fn take_entry(row: &mut Row, usage: &UsageEntry, now: f64) {
    let measured = usage.fetched_at.unwrap_or(now);
    row.bars = (usage.last_good.as_ref())
        .map(|reading| bars(reading, measured))
        .unwrap_or_default();
    row.measured_at = usage.fetched_at;
    row.problem = entry_problem(usage, now);
}

/// `account`, a snapshot's, as the panel's row at `now`.
pub fn row(account: &AccountSnapshot, now: f64) -> Row {
    let slot = &account.slot;
    let mut row = Row {
        number: slot.number.clone(),
        email: slot.identity.email.clone().into(),
        organisation: Some(slot.organization_name.clone())
            .filter(|name| !name.is_empty())
            .map(Into::into),
        plan: (account.plan.as_ref())
            .and_then(|plan| plan.label.clone())
            .map(Into::into),
        active: account.is_active,
        disabled: slot.disabled,
        switchable: slot.has_backup,
        bars: Vec::new(),
        measured_at: None,
        problem: None,
    };
    take_entry(&mut row, &account.usage, now);
    row
}

fn live_window(window: &LimitWindow) -> LiveWindow {
    LiveWindow {
        utilization_fraction: window.utilization,
        resets_at: poll_policy::parse_reset_ts(Some(&window.resets_at.0)),
    }
}

/// Whether `newer` limits moved from `written`: a window came or went, its use by a point or
/// more (`poll_policy::MOVEMENT_DELTA_PCT`, the store's own measure of movement), or its reset.
pub fn moved(written: &LiveLimits, newer: &LiveLimits) -> bool {
    let window = |a: Option<LiveWindow>, b: Option<LiveWindow>| match (a, b) {
        (Some(a), Some(b)) => {
            let used = (a.utilization_fraction - b.utilization_fraction).abs() * 100.;
            let reset = match (a.resets_at, b.resets_at) {
                (Some(a), Some(b)) => (a - b).abs() >= 1.,
                (None, None) => false,
                _ => true,
            };
            used >= poll_policy::MOVEMENT_DELTA_PCT || reset
        }
        (None, None) => false,
        _ => true,
    };
    window(written.five_hour, newer.five_hour)
        || window(written.seven_day, newer.seven_day)
        || window(
            written.seven_day_overage_included,
            newer.seven_day_overage_included,
        )
}

/// A chat's response's limits as the accounts crate takes them: the five hours, the week and
/// the per-model bucket (Fable's `7d_oi`), each a fraction with its reset.
pub fn live_limits(limits: &AccountLimits) -> LiveLimits {
    LiveLimits {
        five_hour: limits.five_hour.as_ref().map(live_window),
        seven_day: limits.seven_day.as_ref().map(live_window),
        seven_day_overage_included: limits.seven_day_overage_included.as_ref().map(live_window),
    }
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
    fn limits_move_by_a_point_a_reset_or_a_window_coming_or_going() {
        let week = |used: f64, resets: f64| LiveWindow {
            utilization_fraction: used,
            resets_at: Some(resets),
        };
        let limits = |seven_day| LiveLimits {
            five_hour: None,
            seven_day,
            seven_day_overage_included: None,
        };
        let written = limits(Some(week(0.40, 1_800_000_000.)));
        assert!(!moved(&written, &limits(Some(week(0.405, 1_800_000_000.)))));
        assert!(moved(&written, &limits(Some(week(0.415, 1_800_000_000.)))));
        assert!(moved(&written, &limits(Some(week(0.40, 1_800_604_800.)))));
        assert!(moved(&written, &limits(None)));
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

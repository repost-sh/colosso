//! What the panel says of a reading (`docs/claude-accounts.md` 4), worked out as it's drawn:
//! the percent ("?" for an unknown, never 0 %; the limit is the bar's red), a threshold, what
//! auto-switch did, a reading's age, and each problem with its remedy.

use koyo_accounts::autoswitch::{AutoSwitchEvent, QuarantineReason, Trigger, pct_label};
use koyo_accounts::oauth::login_expiry_clock;

use super::model::{Bar, Problem, Row, View};

/// What the Mac's login is when Koyo doesn't keep it.
pub const NOT_KEPT: &str = "The Mac's login, not one of Koyo's accounts yet";

/// "72%", or "?" for an unknown: never 0 %.
pub fn percent(bar: &Bar) -> String {
    bar.percent
        .map_or_else(|| "?".to_owned(), |percent| format!("{percent:.0}%"))
}

/// A threshold: "90%", "99.9%", never 99.9 rounded to "100%" (the crate's `pct_label`).
pub fn threshold(threshold: f64) -> String {
    format!("{}%", pct_label(threshold))
}

/// What auto-switch did that the panel says, as of `now`; `None` for what it does every tick.
pub fn auto_switched(event: &AutoSwitchEvent, now: f64) -> Option<String> {
    let at = |ts: f64| login_expiry_clock(ts, now);
    match event {
        AutoSwitchEvent::Switch(switch) => {
            let from = (switch.from_ref.as_ref()).map_or("The Mac's login", |from| &from.email);
            let to = (switch.to_ref.as_ref()).map_or("another account", |to| &to.email);
            let why = match switch.trigger {
                Trigger::Proactive => "reached the threshold",
                Trigger::AtLimit => "hit its limit",
                Trigger::Failover => "couldn't be read",
            };
            Some(format!("{from} {why}: moved to {to} at {}.", at(switch.ts)))
        }
        AutoSwitchEvent::AllExhausted(all) => Some(match all.earliest_reset_at {
            Some(reset) => format!(
                "Every account is at its limit; the first resets {}.",
                at(reset)
            ),
            None => "Every account is at its limit.".to_owned(),
        }),
        AutoSwitchEvent::Quarantine(held) => Some(format!(
            "{} is left out of auto-switch: {}. Sign in to it with Claude Code, then add the \
             current login.",
            held.email,
            match held.reason {
                QuarantineReason::InvalidGrant => "its login has ended",
                QuarantineReason::IdentityConflict => "its login is another account's",
            }
        )),
        AutoSwitchEvent::Error(error) => Some(format!(
            "Auto-switch couldn't run: {}; it tries again.",
            error.message
        )),
        _ => None,
    }
}

/// What the Mac's login is to Koyo, said over Add Current Login: not one of its accounts yet,
/// or the account it is, so an add says it took; nothing with no login.
pub fn mac_login(view: &View) -> Option<String> {
    match (view.unmanaged.as_deref(), view.active()) {
        (Some(email), _) => Some(format!(
            "The Mac's login, {email}, isn't one of Koyo's accounts yet."
        )),
        (None, Some(row)) => Some(format!(
            "The Mac's login, {}, is account {}.",
            row.email, row.number
        )),
        (None, None) => None,
    }
}

/// An account's last line: when the week resets and how old the reading is, or what keeps it
/// from being read.
pub fn status(row: &Row, now: f64) -> String {
    if let Some(problem) = &row.problem {
        return problem.sentence(now);
    }
    let Some(measured) = row.measured_at else {
        return "Not read yet".to_owned();
    };
    let read = age(measured, now);
    match (row.weekly()).and_then(|week| week.as_of(now).resets_at) {
        Some(at) => format!("Resets {} · read {read}", login_expiry_clock(at, now)),
        None => format!("Read {read}"),
    }
}

/// How long ago `at` was.
pub fn age(at: f64, now: f64) -> String {
    crate::ago::ago((now - at).max(0.) as u64)
}

impl Problem {
    /// Whether it needs the person: anything but a wait that passes by itself.
    pub fn needs_you(&self) -> bool {
        !matches!(
            self,
            Problem::RateLimited { .. }
                | Problem::TokenExpired
                | Problem::KeychainUnavailable
                | Problem::Offline
        )
    }

    /// The problem, named with its remedy, as of `now`.
    pub fn sentence(&self, now: f64) -> String {
        match self {
            Problem::RateLimited { until } if *until > now => format!(
                "Rate-limited until {}: Claude's usage page asked Koyo to wait.",
                login_expiry_clock(*until, now)
            ),
            Problem::RateLimited { .. } => {
                "Rate-limited: Claude's usage page asked Koyo to wait.".into()
            }
            Problem::PlanLapsed => "Plan lapsed: this organisation doesn't allow signing in, so its \
                                    usage can't be read. Renewing the plan restores it."
                .into(),
            Problem::CertificateRefused => "Certificate refused: if a proxy re-signs traffic, trust \
                                            its certificate in Keychain Access; otherwise check the \
                                            clock, or for a Wi-Fi sign-in page."
                .into(),
            Problem::SignInAgain => {
                "Sign in again: this login has ended. Sign in with Claude Code (/login).".into()
            }
            Problem::TokenExpired => {
                "Token expired: Claude Code renews it with its next message.".into()
            }
            Problem::KeychainUnavailable => {
                "The Keychain is locked or busy: still signed in. Koyo tries again.".into()
            }
            Problem::ForeignLogin => "The Mac's login belongs to another account: switch to \
                                      this one again to put it back."
                .into(),
            Problem::Offline => "Offline: showing the last reading.".into(),
            Problem::Other(kind) => format!("Couldn't read usage ({kind})."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::model::Provider;

    fn bar(percent: Option<f64>, resets_at: Option<f64>) -> Bar {
        Bar {
            label: "7 days".into(),
            percent,
            resets_at,
            pace: Some(50.),
        }
    }

    #[test]
    fn the_macs_login_is_said_kept_once_added() {
        let mut view = View {
            unmanaged: Some("ana@example.com".into()),
            ..View::default()
        };
        let unkept = mac_login(&view).unwrap();
        assert!(unkept.contains("ana@example.com, isn't one of"), "{unkept}");
        view.unmanaged = None;
        view.rows.push(Row {
            number: "1".to_owned(),
            provider: Provider::Anthropic,
            email: "ana@example.com".into(),
            organisation: None,
            plan: None,
            active: true,
            disabled: false,
            switchable: true,
            bars: Vec::new(),
            measured_at: None,
            problem: None,
        });
        let kept = mac_login(&view).unwrap();
        assert_eq!(kept, "The Mac's login, ana@example.com, is account 1.");
        assert_eq!(mac_login(&View::default()), None);
    }

    #[test]
    fn an_unknown_is_a_question_mark_never_zero() {
        assert_eq!(percent(&bar(None, None)), "?");
        assert_eq!(percent(&bar(Some(0.), None)), "0%");
        assert_eq!(percent(&bar(Some(72.4), None)), "72%");
    }

    #[test]
    fn a_weekly_window_past_its_reset_is_empty_and_rolls_on_by_whole_weeks() {
        use koyo_accounts::pace::WEEKLY_PERIOD_S;
        let now = 1_800_000_000.;
        let before = bar(Some(83.), Some(now + 60.));
        assert_eq!(before.as_of(now), before);
        assert_eq!(before.used(now), Some(83.));
        let passed = bar(Some(83.), Some(now - 10.)).as_of(now);
        assert_eq!(passed.percent, Some(0.));
        assert_eq!(passed.resets_at, Some(now - 10. + WEEKLY_PERIOD_S));
        assert_eq!(passed.pace, None);
        assert_eq!(bar(Some(83.), Some(now - 10.)).used(now), Some(0.));
        let long_ago = bar(Some(83.), Some(now - 2. * WEEKLY_PERIOD_S - 10.)).as_of(now);
        assert_eq!(long_ago.resets_at, Some(now - 10. + WEEKLY_PERIOD_S));
    }

    #[test]
    fn a_rate_limit_says_until_when_while_it_lasts() {
        let now = 1_800_000_000.;
        let limited = Problem::RateLimited { until: now + 600. }.sentence(now);
        assert!(limited.starts_with("Rate-limited until "), "{limited}");
        let lapsed = Problem::RateLimited { until: now - 1. }.sentence(now);
        assert!(lapsed.starts_with("Rate-limited:"), "{lapsed}");
    }
}

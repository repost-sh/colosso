//! What the panel says of a reading (`docs/claude-accounts.md` 4), worked out as it's drawn:
//! the percent ("?" for an unknown, never 0 %), the countdown to a reset and its clock in
//! claude-swap's grammar (the accounts crate's `oauth` formats both), a reading's age, and each
//! problem with its remedy.

use koyo_accounts::oauth::{format_countdown, login_expiry_clock};

use super::model::{Bar, Problem};

/// "72%", "?" for an unknown, with "(!)" once it's at the limit.
pub fn percent(bar: &Bar) -> String {
    match bar.percent {
        None => "?".to_owned(),
        Some(percent) if percent >= 100. => format!("{percent:.0}% (!)"),
        Some(percent) => format!("{percent:.0}%"),
    }
}

/// "resets in 2d 4h (Nov 15 08:59)", or nothing for a window with no reset time.
pub fn resets(bar: &Bar, now: f64) -> Option<String> {
    let at = bar.resets_at?;
    Some(format!(
        "resets in {} ({})",
        format_countdown(at - now),
        login_expiry_clock(at, now)
    ))
}

/// How long ago `at` was.
pub fn age(at: f64, now: f64) -> String {
    crate::ago::ago((now - at).max(0.) as u64)
}

impl Problem {
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
            Problem::Offline => "Offline: showing the last reading.".into(),
            Problem::Other(kind) => format!("Couldn't read usage ({kind})."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(percent: Option<f64>, resets_at: Option<f64>) -> Bar {
        Bar {
            label: "7 days".into(),
            percent,
            resets_at,
            ahead_of_pace: true,
        }
    }

    #[test]
    fn an_unknown_is_a_question_mark_never_zero_and_the_limit_is_marked() {
        assert_eq!(percent(&bar(None, None)), "?");
        assert_eq!(percent(&bar(Some(0.), None)), "0%");
        assert_eq!(percent(&bar(Some(72.4), None)), "72%");
        assert_eq!(percent(&bar(Some(100.), None)), "100% (!)");
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
        assert!(!passed.ahead_of_pace);
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

//! What the button and the panel show of the Mac's login (`docs/claude-accounts.md` 4, 6), read
//! as it was: countdowns and a weekly window past its reset are worked out when drawn
//! (`words.rs`), never stored.

use gpui_kit::SharedString;
use koyo_accounts::pace::WEEKLY_PERIOD_S;

/// One window's bar: the 7 days, or a model's own weekly limit.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// "7 days", or the model's name as the usage endpoint gives it ("Fable").
    pub label: SharedString,
    /// Percent used; `None` for `utilization: null`, shown as "?", never 0 %.
    pub percent: Option<f64>,
    /// When the window resets, in seconds since the epoch.
    pub resets_at: Option<f64>,
    /// Whether the window is used faster than its time passes (claude-swap's pace marker).
    pub ahead_of_pace: bool,
}

impl Bar {
    /// The bar as of `now`: a weekly window whose reset has passed has started again, empty,
    /// and resets a whole number of weeks later (claude-swap 1d38e67).
    pub fn as_of(&self, now: f64) -> Bar {
        match self.resets_at {
            Some(resets) if resets <= now => {
                let weeks = ((now - resets) / WEEKLY_PERIOD_S).floor() + 1.;
                Bar {
                    percent: Some(0.),
                    resets_at: Some(resets + weeks * WEEKLY_PERIOD_S),
                    ahead_of_pace: false,
                    ..self.clone()
                }
            }
            _ => self.clone(),
        }
    }

    /// How much of the window is used as of `now`, past its reset none.
    pub fn used(&self, now: f64) -> Option<f64> {
        if self.resets_at.is_some_and(|resets| resets <= now) {
            Some(0.)
        } else {
            self.percent
        }
    }
}

/// What stands in the way of a reading, named with its remedy (`Problem::sentence`).
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    /// The usage endpoint said to wait until then (seconds since the epoch).
    RateLimited { until: f64 },
    /// The organisation doesn't allow OAuth sign-in: a lapsed plan.
    PlanLapsed,
    /// The TLS certificate was refused: a proxy that re-signs traffic, a portal, the clock.
    CertificateRefused,
    /// The stored login is dead: sign in again.
    SignInAgain,
    /// The access token expired; Claude Code renews it with its next message.
    TokenExpired,
    /// The Keychain was locked or busy: not signed out.
    KeychainUnavailable,
    /// No network, or no answer in time.
    Offline,
    /// Anything else, as the accounts crate names it ("http-500", "bad-response").
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub email: SharedString,
    /// The account's organisation, with its id, which tells two of one email apart.
    pub organisation: Option<SharedString>,
    pub organisation_uuid: String,
    pub active: bool,
    /// The 7 days first, then each model's.
    pub bars: Vec<Bar>,
    /// When the bars were measured, in seconds since the epoch; `None` with no reading yet.
    pub measured_at: Option<f64>,
    pub problem: Option<Problem>,
}

impl Row {
    /// The 7-day bar, if the account has one.
    pub fn weekly(&self) -> Option<&Bar> {
        self.bars.first().filter(|bar| bar.label == WEEKLY)
    }

    /// Whether `other` is the same account.
    pub fn same_account(&self, other: &Row) -> bool {
        self.email == other.email && self.organisation_uuid == other.organisation_uuid
    }
}

/// The 7-day bar's label.
pub const WEEKLY: &str = "7 days";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub rows: Vec<Row>,
    /// Why the login couldn't be read at all.
    pub said: Option<String>,
    /// A variable in the login shell that makes Claude Code ignore the login.
    pub overridden_by: Option<&'static str>,
}

impl View {
    pub fn active(&self) -> Option<&Row> {
        self.rows.iter().find(|row| row.active)
    }
}

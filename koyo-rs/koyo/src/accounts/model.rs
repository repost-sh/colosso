//! What the button and the panel show of the accounts (`docs/claude-accounts.md` 4, 6), from the
//! accounts crate's snapshot as it was: countdowns and a weekly window past its reset are worked
//! out when drawn, never stored.

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
    /// The Mac's login belongs to another account than the one marked active.
    ForeignLogin,
    /// No network, or no answer in time.
    Offline,
    /// Anything else, as the accounts crate names it ("http-500", "bad-response").
    Other(String),
}

/// An account the person acts on: its slot, and its email for what's said of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub number: String,
    pub email: SharedString,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The account's slot, as the accounts crate numbers them.
    pub number: String,
    pub email: SharedString,
    pub organisation: Option<SharedString>,
    /// The plan's label ("Max 20x", "Pro"), when known.
    pub plan: Option<SharedString>,
    /// Whether it's the Mac's login now.
    pub active: bool,
    /// Held out of auto-switch by the person; still one a click switches to.
    pub disabled: bool,
    /// Whether Koyo keeps its login, so a click can switch to it.
    pub switchable: bool,
    /// The 7 days first, then each model's.
    pub bars: Vec<Bar>,
    /// When the bars were measured, in seconds since the epoch; `None` with no reading yet.
    pub measured_at: Option<f64>,
    pub problem: Option<Problem>,
}

impl Row {
    pub fn slot(&self) -> Slot {
        Slot {
            number: self.number.clone(),
            email: self.email.clone(),
        }
    }

    /// The line under its email: organisation, plan, whether it's the Mac's login and whether
    /// it's held out.
    pub fn under(&self) -> String {
        let under = [
            self.organisation.as_deref(),
            self.plan.as_deref(),
            self.active.then_some("the Mac's login"),
            self.disabled.then_some("held out of auto-switch"),
        ];
        under.into_iter().flatten().collect::<Vec<_>>().join(" · ")
    }

    /// The 7-day bar, if the account has one.
    pub fn weekly(&self) -> Option<&Bar> {
        self.bars.first().filter(|bar| bar.label == WEEKLY)
    }
}

/// The 7-day bar's label.
pub const WEEKLY: &str = "7 days";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// Every account Koyo keeps, in its order.
    pub rows: Vec<Row>,
    /// The Mac's login when Koyo doesn't keep it yet: Add Current Login keeps it.
    pub unmanaged: Option<SharedString>,
    /// What's being done now, as "Switching to …".
    pub doing: Option<String>,
    /// What the last thing the person did said: why it failed, or what it warns of.
    pub said: Option<String>,
    /// Why the accounts couldn't be read.
    pub unreadable: Option<String>,
    /// A variable in the login shell that makes Claude Code ignore the login.
    pub overridden_by: Option<&'static str>,
}

impl View {
    pub fn active(&self) -> Option<&Row> {
        self.rows.iter().find(|row| row.active)
    }
}

//! What the button and the panel show of the accounts (`docs/claude-accounts.md` 4, 6), from the
//! accounts crate's snapshot as it was: countdowns and a weekly window past its reset are worked
//! out when drawn, never stored.

use gpui_kit::SharedString;
use koyo_accounts::autoswitch::DEFAULT_THRESHOLD;
use koyo_accounts::pace::{AHEAD_THRESHOLD_PCT, WEEKLY_PERIOD_S};

/// One window's bar: the 7 days, or a model's own weekly limit.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// "7 days", or the model's name as the usage endpoint gives it ("Fable").
    pub label: SharedString,
    /// Percent used; `None` for `utilization: null`, shown as "?", never 0 %.
    pub percent: Option<f64>,
    /// When the window resets, in seconds since the epoch.
    pub resets_at: Option<f64>,
    /// Where even use of the window would have it, in percent, as of the reading as its percent
    /// is (the crate's pace); `None` just after it resets.
    pub pace: Option<f64>,
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
                    pace: None,
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

    /// Whether the window is used well ahead of its time passing (claude-swap's pace marker).
    pub fn ahead_of_pace(&self) -> bool {
        (self.percent.zip(self.pace)).is_some_and(|(used, pace)| used - pace >= AHEAD_THRESHOLD_PCT)
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

/// Whose login an account is: the panel groups the accounts by it, each group under its mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// Claude's: the only logins the accounts crate keeps, and Claude Code's on the Mac.
    Anthropic,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The account's slot, as the accounts crate numbers them.
    pub number: String,
    pub provider: Provider,
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
    /// Auto-switch, while it's on.
    pub auto_switch: Option<AutoSwitch>,
}

/// One provider's accounts, as the panel draws them under its mark.
#[derive(Debug, PartialEq)]
pub struct Group<'a> {
    pub provider: Provider,
    /// Its accounts Koyo keeps, in their order.
    pub rows: Vec<&'a Row>,
    /// The Mac's login when Koyo doesn't keep it and it's this provider's: after the rows.
    pub unmanaged: Option<&'a SharedString>,
}

/// Auto-switch as the panel says it.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoSwitch {
    /// The usage, in percent, at which it moves on.
    pub threshold: f64,
    /// What it last did that matters, as said.
    pub said: Option<String>,
}

impl View {
    pub fn active(&self) -> Option<&Row> {
        self.rows.iter().find(|row| row.active)
    }

    /// The usage auto-switch moves on at, or would if it were on: what a window is coloured
    /// against.
    pub fn line(&self) -> f64 {
        (self.auto_switch.as_ref()).map_or(DEFAULT_THRESHOLD, |auto| auto.threshold)
    }

    /// The accounts by provider, the groups in the order their first account comes. The Mac's
    /// login Koyo doesn't keep is Claude Code's, so it's Anthropic's, last in that group.
    pub fn groups(&self) -> Vec<Group<'_>> {
        let unmanaged = (self.unmanaged.as_ref()).map(|email| (Provider::Anthropic, email));
        let providers = (self.rows.iter().map(|row| row.provider))
            .chain(unmanaged.map(|(provider, _)| provider));
        let mut groups: Vec<Group<'_>> = Vec::new();
        for provider in providers {
            if groups.iter().any(|group| group.provider == provider) {
                continue;
            }
            let rows = (self.rows.iter()).filter(|row| row.provider == provider);
            let login = unmanaged.filter(|(its, _)| *its == provider);
            groups.push(Group {
                provider,
                rows: rows.collect(),
                unmanaged: login.map(|(_, email)| email),
            });
        }
        groups
    }
}

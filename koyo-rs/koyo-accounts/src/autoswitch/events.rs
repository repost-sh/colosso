//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): what a tick reports
//! ([`TickReport`]: its outcome, the wait it asks for and its events), the typed events with
//! the sentence the log shows (`human()`), and the reasons they carry.
//!
//! Each event carries `ts`, the engine clock's instant in epoch seconds, where cswap stamps an
//! ISO string from the wall clock. The sentences name accounts by email (by slot when the email
//! is unknown), and where a cswap sentence tells the person to run a `cswap` command, it names
//! Koyo's own remedy instead (Add Current Login); the reasons stay cswap's strings. Left out:
//! JSON output (`to_json`), `SleepEvent` (the app owns the wait) and `ConfigWarningEvent`
//! (it reported a `model` setting Koyo doesn't have).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lines::BindingWindow;
use crate::oauth::{INVALID_GRANT_ERROR, iso_z};
use crate::switcher::AccountRef;

/// One tick: what it did, how long to wait before the next, and what it reported.
#[derive(Clone, Debug, PartialEq)]
pub struct TickReport {
    pub outcome: TickOutcome,
    /// Seconds.
    pub delay: f64,
    pub events: Vec<AutoSwitchEvent>,
}

/// What one tick did (cswap's values double as `--once` exit codes, left out).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickOutcome {
    Switched,
    Error,
    NoAction,
    /// Wanted to switch but no viable target, or every account exhausted.
    Blocked,
}

/// Why the engine moves: the active account over the line, hard at its limit, or unreadable
/// for `unhealthy_ticks` ticks. cswap's other triggers belong to the strategies left out. The
/// state file's `leftTrigger` stores cswap's string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    Proactive,
    AtLimit,
    Failover,
}

impl Trigger {
    /// cswap's string.
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Proactive => "proactive",
            Trigger::AtLimit => "at-limit",
            Trigger::Failover => "failover",
        }
    }
}

/// Why a tick held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoSwitchReason {
    BelowThreshold,
    Cooldown,
    NoCandidates,
    ActiveIdle,
    ActiveUsageUnknown,
    UnmanagedActiveAccount,
    NoActiveAccount,
    NoComparison,
    NoQualifyingCandidate,
    NoViableTarget,
    AlreadyActive,
}

impl NoSwitchReason {
    /// cswap's string.
    pub fn as_str(self) -> &'static str {
        match self {
            NoSwitchReason::BelowThreshold => "below-threshold",
            NoSwitchReason::Cooldown => "cooldown",
            NoSwitchReason::NoCandidates => "no-candidates",
            NoSwitchReason::ActiveIdle => "active-idle",
            NoSwitchReason::ActiveUsageUnknown => "active-usage-unknown",
            NoSwitchReason::UnmanagedActiveAccount => "unmanaged-active-account",
            NoSwitchReason::NoActiveAccount => "no-active-account",
            NoSwitchReason::NoComparison => "no-comparison",
            NoSwitchReason::NoQualifyingCandidate => "no-qualifying-candidate",
            NoSwitchReason::NoViableTarget => "no-viable-target",
            NoSwitchReason::AlreadyActive => "already-active",
        }
    }
}

/// Why an account was taken out of rotation; the state file keeps the string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuarantineReason {
    /// Its refresh token is dead.
    InvalidGrant,
    /// Its credential authenticates as another account.
    IdentityConflict,
}

impl QuarantineReason {
    /// cswap's string.
    pub fn as_str(self) -> &'static str {
        match self {
            QuarantineReason::InvalidGrant => INVALID_GRANT_ERROR,
            QuarantineReason::IdentityConflict => "identity-conflict",
        }
    }
}

/// Why a quarantined account came back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseReason {
    /// Its refresh token changed: the person signed in again and re-added it.
    CredentialsReplaced,
    /// The slot was removed, or now holds another email.
    AccountReplaced,
}

impl ReleaseReason {
    /// cswap's string.
    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseReason::CredentialsReplaced => "credentials-replaced",
            ReleaseReason::AccountReplaced => "account-replaced",
        }
    }
}

/// One poll: the active account and every account's headroom (`None` unknown), the line, each
/// unknown account's last fetch error ("http-429", "timeout"), each readable account's windows
/// in order (5h, 7d), the active account's binding window with its line, and each account's
/// email. Every pct is raw: the line applies where the engine compares, never to the numbers it
/// reports.
#[derive(Clone, Debug, PartialEq)]
pub struct PollEvent {
    pub ts: f64,
    pub active: Option<AccountRef>,
    pub headroom: BTreeMap<String, Option<f64>>,
    pub threshold: f64,
    pub fetch_errors: BTreeMap<String, String>,
    pub windows: BTreeMap<String, Vec<(String, f64)>>,
    pub binding: Option<BindingWindow>,
    pub emails: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchEvent {
    pub ts: f64,
    pub trigger: Trigger,
    pub from_ref: Option<AccountRef>,
    pub to_ref: Option<AccountRef>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoSwitchEvent {
    pub ts: f64,
    pub reason: NoSwitchReason,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuarantineEvent {
    pub ts: f64,
    pub number: String,
    pub email: String,
    pub reason: QuarantineReason,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnquarantineEvent {
    pub ts: f64,
    pub number: String,
    pub email: String,
    pub reason: ReleaseReason,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AllExhaustedEvent {
    pub ts: f64,
    /// Epoch seconds.
    pub earliest_reset_at: Option<f64>,
}

/// A tick that failed; the next one retries.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorEvent {
    pub ts: f64,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AutoSwitchEvent {
    Poll(PollEvent),
    Switch(SwitchEvent),
    NoSwitch(NoSwitchEvent),
    Quarantine(QuarantineEvent),
    Unquarantine(UnquarantineEvent),
    AllExhausted(AllExhaustedEvent),
    Error(ErrorEvent),
}

impl AutoSwitchEvent {
    /// cswap's `kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            AutoSwitchEvent::Poll(_) => "poll",
            AutoSwitchEvent::Switch(_) => "switch",
            AutoSwitchEvent::NoSwitch(_) => "no-switch",
            AutoSwitchEvent::Quarantine(_) => "account-quarantined",
            AutoSwitchEvent::Unquarantine(_) => "account-unquarantined",
            AutoSwitchEvent::AllExhausted(_) => "all-exhausted",
            AutoSwitchEvent::Error(_) => "error",
        }
    }

    /// Epoch seconds.
    pub fn ts(&self) -> f64 {
        match self {
            AutoSwitchEvent::Poll(event) => event.ts,
            AutoSwitchEvent::Switch(event) => event.ts,
            AutoSwitchEvent::NoSwitch(event) => event.ts,
            AutoSwitchEvent::Quarantine(event) => event.ts,
            AutoSwitchEvent::Unquarantine(event) => event.ts,
            AutoSwitchEvent::AllExhausted(event) => event.ts,
            AutoSwitchEvent::Error(event) => event.ts,
        }
    }

    /// The sentence the log shows.
    pub fn human(&self) -> String {
        match self {
            AutoSwitchEvent::Poll(event) => event.human(),
            AutoSwitchEvent::Switch(event) => {
                let src = event
                    .from_ref
                    .as_ref()
                    .map_or_else(|| "(none)".to_owned(), name);
                let dst = event.to_ref.as_ref().map_or_else(|| "?".to_owned(), name);
                format!("Switched {src} -> {dst} ({})", event.trigger.as_str())
            }
            AutoSwitchEvent::NoSwitch(event) if event.detail.is_empty() => {
                format!("no switch: {}", event.reason.as_str())
            }
            AutoSwitchEvent::NoSwitch(event) => {
                format!("no switch: {} ({})", event.reason.as_str(), event.detail)
            }
            AutoSwitchEvent::Quarantine(event) => format!(
                "{} quarantined: {}. Sign in to it with Claude Code, then Add Current Login to \
                 recover.",
                slot_name(&event.number, &event.email),
                event.reason.as_str()
            ),
            AutoSwitchEvent::Unquarantine(event) => format!(
                "{} back in rotation ({})",
                slot_name(&event.number, &event.email),
                event.reason.as_str()
            ),
            AutoSwitchEvent::AllExhausted(event) => match event.earliest_reset_at.and_then(iso_z) {
                Some(at) => format!("all accounts exhausted; earliest reset {at}"),
                None => "all accounts exhausted; no reset time known".to_owned(),
            },
            AutoSwitchEvent::Error(event) => format!("error: {} (will retry)", event.message),
        }
    }
}

/// An account as a sentence names it: its email, else `Account-<n>`.
fn slot_name(number: &str, email: &str) -> String {
    if email.is_empty() {
        format!("Account-{number}")
    } else {
        email.to_owned()
    }
}

fn name(account: &AccountRef) -> String {
    let number = account.number.map(|number| number.to_string());
    slot_name(number.as_deref().unwrap_or("?"), &account.email)
}

/// Slot numbers in their numeric order, as cswap's sequence lists them.
fn slot_order<'a>(slots: impl Iterator<Item = &'a String>) -> Vec<&'a String> {
    let mut slots: Vec<&String> = slots.collect();
    slots.sort_by_key(|slot| (slot.parse::<u64>().unwrap_or(u64::MAX), slot.as_str()));
    slots
}

impl PollEvent {
    fn describe(&self, num: &str) -> String {
        if let Some(windows) = self.windows.get(num).filter(|windows| !windows.is_empty()) {
            return windows
                .iter()
                .map(|(name, pct)| format!("{name} {pct:.0}%"))
                .collect::<Vec<_>>()
                .join(" · ");
        }
        if let Some(Some(headroom)) = self.headroom.get(num) {
            return format!("{:.0}%", 100. - headroom);
        }
        match self.fetch_errors.get(num) {
            Some(error) => format!("? ({error})"),
            None => "?".to_owned(),
        }
    }

    pub fn human(&self) -> String {
        let Some(active) = &self.active else {
            return "poll: no active account".to_owned();
        };
        let num = active.number.map(|number| number.to_string());
        let num = num.as_deref().unwrap_or_default();
        // The binding window and its line, the engine's own comparison: the bare pct hid which
        // window bound (#115).
        let (used, line) = if let Some(binding) = &self.binding {
            (
                format!("{} {:.0}% used", binding.label, binding.pct),
                binding.line,
            )
        } else if let Some(Some(headroom)) = self.headroom.get(num) {
            (format!("{:.0}% used", 100. - headroom), self.threshold)
        } else {
            let used = match self.fetch_errors.get(num) {
                Some(error) => format!("usage unknown ({error})"),
                None => "usage unknown".to_owned(),
            };
            (used, self.threshold)
        };
        let others: Vec<String> = slot_order(self.headroom.keys())
            .into_iter()
            .filter(|other| *other != num)
            .map(|other| {
                let email = self.emails.get(other).map_or("", String::as_str);
                format!("{}: {}", slot_name(other, email), self.describe(other))
            })
            .collect();
        let tail = if others.is_empty() {
            String::new()
        } else {
            format!(" | others: {}", others.join(", "))
        };
        format!(
            "{}: {used} (switch at {}%){tail}",
            name(active),
            pct_label(line)
        )
    }
}

/// A percentage for display, as configured: 85.555555 stays itself (never a rounded
/// "85.5556") and 99.9 never becomes a lying "100" the way `.0f` renders it; ten significant
/// digits still absorb float noise (~15th digit) in computed utilizations (100 - headroom).
/// Python's `f"{value:.10g}"` over the range a percentage takes; outside it (1e10 and more,
/// under 1e-4) the digits stay fixed-point where Python's switch to an exponent. A displayed
/// comparison formats both sides with it: mixing formatters can render an impossible
/// "85.5556% < 85.555555%".
pub fn pct_label(value: f64) -> String {
    // The exponent after rounding to ten significant digits, then the decimals that leaves.
    let exponent: i32 = format!("{value:.9e}")
        .split_once('e')
        .and_then(|(_, exponent)| exponent.parse().ok())
        .unwrap_or(0);
    let decimals = usize::try_from(9 - exponent).unwrap_or(0);
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// The below-threshold hold, quoting the comparison the engine made: the binding window's own
/// pct against its line ("5h 50% < 90%").
pub(crate) fn below_line_detail(binding: &BindingWindow) -> String {
    format!(
        "{} {}% < {}%",
        binding.label,
        pct_label(binding.pct),
        pct_label(binding.line)
    )
}

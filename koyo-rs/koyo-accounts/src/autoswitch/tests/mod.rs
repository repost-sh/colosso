//! Tests translated from claude-swap's `tests/test_autoswitch.py`,
//! `tests/test_autoswitch_landing_room.py` and `tests/test_weekly_only_strategies.py` (0fe22a8),
//! each named after cswap's. cswap's `EngineHarness` seeds a real switcher on its file backend
//! and patches `usage_entries_by_account`; here it's [`Harness`] over [`FakeHost`]: accounts with
//! their credentials, the entries every pass returns, scripted grant outcomes, and a log of
//! fetches, grants and switches. The clock starts where cswap's `FakeClock` does (1,000,000), so
//! cswap's absolute reset dates keep their place relative to now.
//!
//! cswap's harness runs `best`; the engine runs weekly-first, which ranks every path those tests
//! take as `best` does unless a candidate's room or weekly reset sets them apart, and none of the
//! translated cases depends on that. Tests of what isn't ported are skipped, listed in the port's
//! report: since the simplify pass, those of the weekly line, the per-model windows and the
//! model-name check, of a dry run, of `stop`/`wake` and the sleep event, and of state records
//! written before a field existed.

mod all_above;
mod decision;
mod direct;
mod events;
mod fake;
mod freshen;
mod idle_hold;
mod loop_delay;
mod model;
mod no_return;
mod no_return_release;
mod no_return_walks;
mod quarantine;
mod release_predicate;
mod scheduler;
mod weekly_first;
mod weekly_threshold;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};

use super::state::{AutoSwitchState, Quarantined};
use super::*;
use crate::env::Clock as _;
use crate::oauth::{USAGE_TOKEN_EXPIRED, credential_fingerprint};
use crate::switcher::{AccountRef, SlotIdentity};
use crate::testing::{Sandbox, iso, usage};
use crate::usage_store::UsageEntry;

use direct::Direct;
use fake::{Account, FakeHost, credentials};

/// cswap's `FakeClock` start.
pub(super) const START: f64 = 1_000_000.;

/// Weekly-reset instants in ascending order, far in the future of [`START`].
pub(super) const R_SOON: &str = "2024-01-05T00:00:00Z";
pub(super) const R_LATER: &str = "2024-01-08T00:00:00Z";
pub(super) const R_LATEST: &str = "2024-01-10T00:00:00Z";

// -- usage -----------------------------------------------------------------------------------

/// `_usage` with the 5h window resetting at `resets_at`.
pub(super) fn usage_r(pct: f64, resets_at: &str) -> Value {
    json!({"five_hour": {"pct": pct, "resets_at": resets_at}, "seven_day": {"pct": 0.0}})
}

/// `_usage` resetting `seconds` from now.
pub(super) fn r(pct: f64, h: &Harness, seconds: f64) -> Value {
    usage_r(pct, &at(h, seconds))
}

/// cswap's `_usage7`: both windows, the 7d one resetting at `reset7`.
pub(super) fn usage7(pct5: f64, pct7: f64, reset7: Option<&str>) -> Value {
    usage57(pct5, None, pct7, reset7)
}

/// cswap's `_usage57`: both windows with optional resets.
pub(super) fn usage57(pct5: f64, reset5: Option<&str>, pct7: f64, reset7: Option<&str>) -> Value {
    let window = |pct: f64, resets_at: Option<&str>| {
        let mut window = json!({"pct": pct});
        if let Some(resets_at) = resets_at {
            window["resets_at"] = json!(resets_at);
        }
        window
    };
    json!({"five_hour": window(pct5, reset5), "seven_day": window(pct7, reset7)})
}

/// A reading with only a 7d window (an Ultra account), resetting `reset_in` from now.
pub(super) fn weekly(h: &Harness, pct: f64, reset_in: Option<f64>) -> Value {
    let mut window = json!({"pct": pct});
    if let Some(reset_in) = reset_in {
        window["resets_at"] = json!(at(h, reset_in));
    }
    json!({"seven_day": window})
}

/// The active token expired while Claude Code owns it, two healthy peers.
pub(super) fn held() -> [(&'static str, Value); 3] {
    [
        ("1", json!(USAGE_TOKEN_EXPIRED)),
        ("2", usage(10.)),
        ("3", usage(20.)),
    ]
}

/// cswap's `_at`: `seconds` from now.
pub(super) fn at(h: &Harness, seconds: f64) -> String {
    iso(h.now() + seconds)
}

/// cswap's `_entry_for`: the store entry a live fetch would have produced. A JSON object is a
/// reading, a string a sentinel, `null` unknown.
pub(super) fn entry_for(value: &Value, now: f64) -> UsageEntry {
    match value {
        Value::Object(_) => UsageEntry {
            last_good: Some(value.clone()),
            fetched_at: Some(now),
            age_s: Some(0.),
            ..UsageEntry::default()
        },
        Value::String(sentinel) => UsageEntry {
            sentinel: Some(sentinel.clone()),
            ..UsageEntry::default()
        },
        _ => UsageEntry::default(),
    }
}

// -- settings --------------------------------------------------------------------------------

/// cswap's defaults.
pub(super) fn defaults() -> AutoSwitchSettings {
    AutoSwitchSettings::new(DEFAULT_THRESHOLD)
}

/// Settings with the given threshold, cswap's defaults otherwise.
pub(super) fn threshold(threshold: f64) -> AutoSwitchSettings {
    AutoSwitchSettings::new(threshold)
}

// -- events ----------------------------------------------------------------------------------

/// An event type, picked out of [`AutoSwitchEvent`].
pub(super) trait Pick: Sized {
    fn pick(event: AutoSwitchEvent) -> Option<Self>;
}

macro_rules! pick {
    ($($variant:ident => $event:ty),* $(,)?) => {$(
        impl Pick for $event {
            fn pick(event: AutoSwitchEvent) -> Option<$event> {
                match event {
                    AutoSwitchEvent::$variant(event) => Some(event),
                    _ => None,
                }
            }
        }
    )*};
}

pick!(
    Poll => PollEvent,
    Switch => SwitchEvent,
    NoSwitch => NoSwitchEvent,
    Quarantine => QuarantineEvent,
    AllExhausted => AllExhaustedEvent,
    Error => ErrorEvent,
);

// -- the harness -----------------------------------------------------------------------------

/// cswap's `EngineHarness`: a sandbox, the fake switcher, the settings and the captured events.
pub(super) struct Harness {
    pub sandbox: Sandbox,
    pub env: Env,
    pub host: Arc<FakeHost>,
    pub settings: AutoSwitchSettings,
    pub events: Vec<AutoSwitchEvent>,
    /// The last tick's wait.
    pub delay: f64,
    pub engine: AutoSwitchEngine,
}

impl Harness {
    pub fn new(settings: AutoSwitchSettings) -> Harness {
        let sandbox = Sandbox::new();
        sandbox.clock.set(START);
        let env = sandbox.env();
        let host = Arc::new(FakeHost::new(sandbox.clock.clone()));
        let engine = make_engine(&env, &host, &settings);
        Harness {
            sandbox,
            env,
            host,
            settings,
            events: Vec::new(),
            delay: 0.,
            engine,
        }
    }

    /// cswap's `harness` fixture: accounts 1-3 (a@, b@, c@example.com), 1 live.
    pub fn standard() -> Harness {
        Harness::fleet(3, defaults())
    }

    /// `n` accounts under `settings`, 1 live, emails a@, b@, ... as cswap's fixtures name them.
    pub fn fleet(n: u32, settings: AutoSwitchSettings) -> Harness {
        Harness::named(n, settings, email)
    }

    /// cswap's `_harness`: `n` accounts named `<n>@example.com`, 1 live.
    pub fn numbered(n: u32, settings: AutoSwitchSettings) -> Harness {
        Harness::named(n, settings, |num| format!("{num}@example.com"))
    }

    fn named(n: u32, settings: AutoSwitchSettings, name: fn(u32) -> String) -> Harness {
        let harness = Harness::new(settings);
        for num in 1..=n {
            harness.seed(num, &name(num));
        }
        harness.make_live(1);
        harness
    }

    /// cswap's `seed`.
    pub fn seed(&self, num: u32, email: &str) {
        self.seed_expiring(num, email, None);
    }

    pub fn seed_expiring(&self, num: u32, email: &str, expires_at: Option<f64>) {
        self.host.lock().accounts.insert(
            num,
            Account {
                email: email.to_owned(),
                credentials: credentials(&num.to_string(), expires_at),
                uuid: format!("uuid-{num}"),
                org: String::new(),
                disabled: false,
            },
        );
    }

    /// cswap's `make_live`: the Mac's login is this slot's.
    pub fn make_live(&self, num: u32) {
        self.host.lock().current = Some(num.to_string());
    }

    /// A fresh engine on the same store (cswap's `_make_engine()`).
    pub fn renew(&mut self) {
        self.engine = make_engine(&self.env, &self.host, &self.settings);
    }

    pub fn now(&self) -> f64 {
        self.sandbox.clock.now()
    }

    pub fn advance(&self, seconds: f64) {
        self.sandbox.clock.advance(seconds);
    }

    /// One tick, its events captured.
    pub fn tick(&mut self) -> TickOutcome {
        let report = self.engine.tick();
        self.events.extend(report.events);
        self.delay = report.delay;
        report.outcome
    }

    pub fn tick_with_usage(&mut self, usage: &[(&str, Value)]) -> TickOutcome {
        let now = self.now();
        let entries = usage
            .iter()
            .map(|(num, value)| ((*num).to_owned(), entry_for(value, now)))
            .collect();
        self.tick_with_entries(entries)
    }

    pub fn tick_with_entries(&mut self, entries: BTreeMap<String, UsageEntry>) -> TickOutcome {
        self.host.lock().entries = entries;
        self.tick()
    }

    pub fn active(&self) -> Option<u32> {
        self.host
            .lock()
            .current
            .as_ref()
            .and_then(|c| c.parse().ok())
    }

    pub fn events(&self) -> Vec<AutoSwitchEvent> {
        self.events.clone()
    }

    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    pub fn kinds(&self) -> Vec<&'static str> {
        self.events.iter().map(AutoSwitchEvent::kind).collect()
    }

    /// Every event of one type, in order.
    pub fn all<T: Pick>(&self) -> Vec<T> {
        self.events().into_iter().filter_map(T::pick).collect()
    }

    pub fn reasons(&self) -> Vec<&'static str> {
        self.all::<NoSwitchEvent>()
            .into_iter()
            .map(|event| event.reason.as_str())
            .collect()
    }

    /// The first switch event's trigger.
    pub fn trigger(&self) -> Trigger {
        self.all::<SwitchEvent>().first().expect("a switch").trigger
    }

    /// cswap's `state()`: the state file as the engine reads it, `{}` when absent.
    pub fn state(&self) -> Value {
        serde_json::to_value(self.engine.read_state()).expect("serializable")
    }

    /// `engine._mutate_state(lambda s: s.update(...))`, on the file's JSON.
    pub fn update_state(&self, values: &Value) {
        self.edit_state(|state| {
            let mut merged = serde_json::to_value(&*state).expect("serializable");
            let merged_map = merged.as_object_mut().expect("an object");
            merged_map.extend(values.as_object().expect("an object").clone());
            *state = serde_json::from_value(merged).expect("a state");
        });
    }

    /// `engine._mutate_state(mutator)`.
    pub fn edit_state(&self, mutator: impl FnOnce(&mut AutoSwitchState)) {
        self.engine
            .with_state(|state| {
                mutator(state);
                Ok(())
            })
            .expect("state written");
    }

    /// `engine._quarantine(number, email, reason)`, on the slot's stored credential.
    pub fn quarantine(&mut self, num: u32, reason: QuarantineReason) {
        let account = self.host.lock().accounts[&num].clone();
        self.engine
            .quarantine(vec![Quarantined {
                number: num.to_string(),
                email: account.email,
                reason,
                fingerprint: credential_fingerprint(&account.credentials),
            }])
            .expect("quarantined");
        self.events.append(&mut self.engine.events);
    }

    /// The slot's identity, as the roster reads it.
    pub fn identity(&self, num: &str) -> SlotIdentity {
        let roster = self.host.roster().expect("a roster");
        roster.slot(num).expect("a slot").identity.clone()
    }
}

/// cswap's fixture emails: a@, b@, c@ … example.com.
pub(super) fn email(num: u32) -> String {
    let letter = char::from(b'a' + (num - 1) as u8);
    format!("{letter}@example.com")
}

fn make_engine(env: &Env, host: &Arc<FakeHost>, settings: &AutoSwitchSettings) -> AutoSwitchEngine {
    let mut engine = AutoSwitchEngine::new(env, host.clone(), settings.clone());
    engine.rng = || 0.5;
    engine
}

/// cswap's exact float assertions, bit for bit.
#[track_caller]
pub(super) fn assert_exact(actual: f64, expected: f64) {
    assert!(
        actual.to_bits() == expected.to_bits(),
        "{actual} != {expected}"
    );
}

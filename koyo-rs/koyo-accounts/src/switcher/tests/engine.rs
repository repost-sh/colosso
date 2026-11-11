//! The auto-switch engine on a real switcher: one `tick` end to end through
//! `impl EngineHost for Switcher` (`switcher/engine_host.rs`), where the engine's own tests
//! run on a fake host. cswap's `EngineHarness` seeds a real switcher the same way; here two
//! accounts are seeded on the sandbox, their usage recorded in the store, and the live login
//! the fake Keychain's `Claude Code-credentials` item.

use std::sync::Arc;

use serde_json::{Value, json};

use super::support::{fresh, recorded, usage_requests};
use super::{Harness, make_live, seed};
use crate::autoswitch::{AutoSwitchEngine, AutoSwitchEvent, AutoSwitchSettings, TickOutcome};
use crate::oauth::OAUTH_TOKEN_URL;
use crate::switcher::{AccountRef, Switcher};
use crate::usage_store::one_identity;

const A: &str = "a@example.com";
const B: &str = "b@example.com";

/// A stored reading with the 5h window at `pct` and the weekly one empty.
fn usage(pct: f64) -> Value {
    json!({"five_hour": {"pct": pct}, "seven_day": {"pct": 0.0}})
}

/// Slots 1 (live) and 2, slot 2's backup good for years so a freshen posts nothing, and each
/// one's usage recorded a minute ago.
fn two_accounts(active_pct: f64, other_pct: f64) -> Harness {
    let h = Harness::new();
    seed(&h, 1, A);
    seed(&h, 2, B);
    h.set_backup("2", B, &fresh("sk-2", "rt-2"));
    make_live(&h, A, 1);
    recorded(&h, "1", usage(active_pct), &one_identity("1", A, ""), 60.);
    recorded(&h, "2", usage(other_pct), &one_identity("2", B, ""), 60.);
    h
}

/// An engine at cswap's default threshold (90) on a switcher of `h`'s sandbox.
fn engine(h: &Harness) -> AutoSwitchEngine {
    let env = h.sandbox.env();
    let switcher = Arc::new(Switcher::new(env.clone()));
    AutoSwitchEngine::new(&env, switcher, AutoSwitchSettings::new(90.))
}

#[test]
fn a_tick_over_the_line_switches_the_mac_to_the_other_account() {
    let h = two_accounts(95., 10.);
    let report = engine(&h).tick();

    assert_eq!(report.outcome, TickOutcome::Switched, "{:?}", report.events);
    assert_eq!(h.live(), Some(fresh("sk-2", "rt-2")));
    assert_eq!(h.config()["oauthAccount"]["emailAddress"], B);
    let switches: Vec<_> = report
        .events
        .iter()
        .filter_map(|event| match event {
            AutoSwitchEvent::Switch(event) => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(switches.len(), 1, "{:?}", report.events);
    assert_eq!(switches[0].from_ref, Some(AccountRef::new(Some(1), A)));
    assert_eq!(switches[0].to_ref, Some(AccountRef::new(Some(2), B)));
    // The target's token was fresh: nothing was spent to activate it.
    assert!(h.sandbox.http.requests_to(OAUTH_TOKEN_URL).is_empty());
}

#[test]
fn a_tick_under_the_line_does_nothing() {
    let h = two_accounts(50., 10.);
    let keychain = h.sandbox.security.items();
    let config = std::fs::read_to_string(h.config_path()).unwrap();
    let sequence = h.sequence();

    let report = engine(&h).tick();

    assert_eq!(report.outcome, TickOutcome::NoAction, "{:?}", report.events);
    assert!(
        !report
            .events
            .iter()
            .any(|event| matches!(event, AutoSwitchEvent::Switch(_))),
        "{:?}",
        report.events
    );
    assert_eq!(h.sandbox.security.items(), keychain);
    assert_eq!(std::fs::read_to_string(h.config_path()).unwrap(), config);
    assert_eq!(h.sequence(), sequence);
    assert_eq!(usage_requests(&h), 0);
    assert!(h.sandbox.http.requests_to(OAUTH_TOKEN_URL).is_empty());
}

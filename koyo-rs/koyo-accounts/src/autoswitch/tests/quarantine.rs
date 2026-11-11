//! cswap's `TestQuarantineLifecycle` and `TestDryRunAndNoOp` (its no-op; a dry run is left out).

use serde_json::json;

use super::*;
use crate::switcher::{SwitchReason, SwitchResult};

fn replace_credentials(h: &Harness, num: u32, access: &str, refresh: &str) {
    h.host.lock().accounts.get_mut(&num).unwrap().credentials =
        json!({"claudeAiOauth": {"accessToken": access, "refreshToken": refresh}}).to_string();
}

#[test]
fn quarantine_persists_across_engine_instances() {
    let mut h = Harness::standard();
    h.quarantine(2, QuarantineReason::InvalidGrant);
    h.clear_events();
    h.renew();
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(0.)), ("3", usage(50.))]);
    // 2 has the most headroom but is quarantined: 3 wins.
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn replaced_credentials_lift_quarantine() {
    let mut h = Harness::standard();
    h.quarantine(2, QuarantineReason::InvalidGrant);
    // The person signed in again and re-added the slot: a new refresh token.
    replace_credentials(&h, 2, "sk-2b", "rt-2b");
    h.clear_events();
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(0.)), ("3", usage(50.))]);
    assert!(h.kinds().contains(&"account-unquarantined"));
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    assert!(h.state()["quarantine"].get("2").is_none());
}

#[test]
fn state_lock_preserves_concurrent_writes() {
    // Another engine writing between our read and our write: the read-modify-write under the
    // state lock keeps its quarantine entry.
    let h = Harness::standard();
    h.update_state(&json!({"quarantine": {"3": {
        "email": "c@example.com", "reason": "invalid_grant", "at": "x",
        "refreshTokenFingerprint": null,
    }}}));
    h.update_state(&json!({"lastSwitchAt": 123.0}));
    let state = h.state();
    assert_eq!(state["lastSwitchAt"], 123.0);
    assert!(state["quarantine"].get("3").is_some());
}

#[test]
fn already_active_result_is_noop() {
    let mut h = Harness::standard();
    h.host.lock().switch_answer = Some(SwitchResult {
        switched: false,
        from: None,
        to: None,
        reason: SwitchReason::AlreadyActive,
        message: String::new(),
        warnings: Vec::new(),
    });
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.)), ("3", usage(50.))]);
    assert_eq!(outcome, TickOutcome::NoAction);
    assert!(h.state().get("lastSwitchAt").is_none());
}

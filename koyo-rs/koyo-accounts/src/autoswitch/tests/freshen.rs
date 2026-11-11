//! cswap's `TestFreshening`, `TestTokenIdentity` and `TestFreshenRoutesThroughGate`. cswap
//! patches the refresh POST under the real switcher's gate; here the fake gate answers what a
//! test scripts, and persists a successor as the real gate does.

use super::super::perform::{Freshen, Systemic};
use super::*;
use crate::oauth::{AccountIdentity, DETERMINISTIC_REFRESH_ERRORS, RefreshOutcome};
use crate::testing::creds;

/// cswap's status strings: a systemic refusal as the gate's kind that reads back to it.
fn kind(status: &Freshen) -> &'static str {
    match status {
        Freshen::Ok => "ok",
        Freshen::Quarantine(reason, _) => reason.as_str(),
        Freshen::Transient => "transient",
        Freshen::Systemic(systemic) => DETERMINISTIC_REFRESH_ERRORS
            .into_iter()
            .find(|kind| Systemic::from_kind(kind) == Some(*systemic))
            .expect("every systemic refusal is one of the gate's kinds"),
    }
}

fn freshen(h: &Harness, num: &str) -> &'static str {
    kind(&h.engine.freshen_target(num, &h.identity(num)))
}

fn script(h: &Harness, num: &str, outcomes: Vec<RefreshOutcome>) {
    h.host.lock().grants.insert(num.to_owned(), outcomes.into());
}

fn rotated(num: &str, expires_at: f64) -> String {
    creds(
        &format!("sk-{num}-new"),
        &format!("rt-{num}-new"),
        Some(expires_at.into()),
    )
}

fn granted(credentials: &str, identity: Option<AccountIdentity>) -> RefreshOutcome {
    RefreshOutcome {
        credentials: Some(credentials.to_owned()),
        token_account: identity,
        ..RefreshOutcome::default()
    }
}

fn identity(uuid: &str, email: &str, org: &str) -> AccountIdentity {
    AccountIdentity {
        uuid: uuid.into(),
        email: Some(email.into()),
        organization_uuid: Some(org.into()),
    }
}

fn credentials_of(h: &Harness, num: u32) -> String {
    h.host.lock().accounts[&num].credentials.clone()
}

/// cswap's `harness` with slot 2 near expiry, so the freshen path runs.
fn expiring_two() -> Harness {
    let h = Harness::standard();
    h.seed_expiring(2, "b@example.com", Some(0.));
    h
}

// -- TestFreshening --------------------------------------------------------------------------

#[test]
fn near_expiry_target_is_refreshed_and_persisted() {
    let mut h = Harness::new(defaults());
    h.seed(1, "a@example.com");
    h.seed_expiring(2, "b@example.com", Some(h.now() * 1000. + 60_000.));
    h.make_live(1);
    let fresh = rotated("2", h.now() * 1000. + 3_600_000.);
    script(&h, "2", vec![granted(&fresh, None)]);
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.host.lock().grant_log, ["2"]);
    // The rotated token went through the slot's backup, which the switch then activated.
    assert!(credentials_of(&h, 2).contains("sk-2-new"));
    assert_eq!(h.active(), Some(2));
}

#[test]
fn fresh_target_is_not_refreshed() {
    let mut h = Harness::new(defaults());
    h.seed(1, "a@example.com");
    h.seed_expiring(2, "b@example.com", Some(h.now() * 1000. + 3_600_000.));
    h.make_live(1);
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert!(h.host.lock().grant_log.is_empty());
}

#[test]
fn invalid_grant_quarantines_and_tries_next() {
    let mut h = Harness::new(defaults());
    h.seed(1, "a@example.com");
    h.seed_expiring(2, "b@example.com", Some(1.)); // long expired
    h.seed(3, "c@example.com");
    h.make_live(1);
    script(&h, "2", vec![RefreshOutcome::failed("invalid_grant")]);
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.)), ("3", usage(20.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3)); // the next candidate after 2 was quarantined
    let quarantined = h.all::<QuarantineEvent>().remove(0);
    assert_eq!(
        (quarantined.number.as_str(), quarantined.reason),
        ("2", QuarantineReason::InvalidGrant)
    );
    assert!(h.state()["quarantine"].get("2").is_some());
}

#[test]
fn transient_failure_skips_without_quarantine() {
    let mut h = Harness::new(defaults());
    h.seed(1, "a@example.com");
    h.seed_expiring(2, "b@example.com", Some(1.));
    h.make_live(1);
    script(&h, "2", vec![RefreshOutcome::failed("transient")]);
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.))]);
    assert_eq!(outcome, TickOutcome::Error);
    assert_eq!(h.active(), Some(1));
    assert!(h.state().get("quarantine").is_none());
    assert!(!h.all::<ErrorEvent>().is_empty());
}

// -- TestTokenIdentity -----------------------------------------------------------------------

#[test]
fn uuid_backfill_from_token_account_on_freshen() {
    let h = expiring_two();
    h.host.lock().accounts.get_mut(&2).unwrap().uuid = String::new();
    let fresh = rotated("2", 99_999_999_999_000.);
    script(
        &h,
        "2",
        vec![granted(
            &fresh,
            Some(identity("uuid-2-real", "b@example.com", "")),
        )],
    );
    assert_eq!(freshen(&h, "2"), "ok");
    assert_eq!(h.host.lock().accounts[&2].uuid, "uuid-2-real");
}

#[test]
fn conflicting_token_identity_returns_identity_conflict() {
    // Not a viable target, but the rotated generation is still persisted: the grant consumed
    // its predecessor.
    let h = expiring_two();
    let fresh = rotated("2", 99_999_999_999_000.);
    script(
        &h,
        "2",
        vec![granted(
            &fresh,
            Some(identity("uuid-somebody-else", "z@example.com", "")),
        )],
    );
    assert_eq!(freshen(&h, "2"), "identity-conflict");
    assert_eq!(credentials_of(&h, 2), fresh);
}

#[test]
fn identity_conflict_quarantines_instead_of_activating() {
    let mut h = expiring_two();
    let fresh = rotated("2", 99_999_999_999_000.);
    script(
        &h,
        "2",
        vec![granted(
            &fresh,
            Some(identity("uuid-somebody-else", "z@example.com", "")),
        )],
    );
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.)), ("3", usage(80.))]);
    // 2 had the most headroom but is conflicted: quarantined, and the switch landed elsewhere.
    assert!(h.kinds().contains(&"account-quarantined"));
    assert_eq!(h.state()["quarantine"]["2"]["reason"], "identity-conflict");
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn an_identity_conflict_stays_quarantined_on_the_next_tick() {
    // The quarantine records the successor the gate persisted, so the next tick doesn't read
    // the rotation as a re-add, release the slot and switch onto the wrong account.
    let mut h = expiring_two();
    let fresh = rotated("2", 99_999_999_999_000.);
    script(
        &h,
        "2",
        vec![granted(
            &fresh,
            Some(identity("uuid-somebody-else", "z@example.com", "")),
        )],
    );
    let fleet = [("1", usage(95.)), ("2", usage(10.)), ("3", usage(80.))];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    h.make_live(1);
    h.advance(400.);
    h.clear_events();
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert!(
        !h.kinds().contains(&"account-unquarantined"),
        "{:?}",
        h.kinds()
    );
    assert_eq!(h.active(), Some(3));
}

#[test]
fn an_invalid_grant_records_the_generation_the_gate_sent() {
    // The gate may adopt a stashed successor into the slot before its POST: the quarantine keys
    // on the generation it sent, so the next tick doesn't take the adoption for a re-add.
    let mut h = expiring_two();
    let adopted = rotated("2", 1.);
    script(
        &h,
        "2",
        vec![RefreshOutcome {
            // The fake gate writes these to the slot, as the adoption did.
            credentials: Some(adopted.clone()),
            consumed_fp: credential_fingerprint(&adopted),
            ..RefreshOutcome::failed("invalid_grant")
        }],
    );
    let fleet = [("1", usage(95.)), ("2", usage(10.)), ("3", usage(80.))];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
    h.make_live(1);
    h.advance(400.);
    h.clear_events();
    h.tick_with_usage(&fleet);
    assert!(
        !h.kinds().contains(&"account-unquarantined"),
        "{:?}",
        h.kinds()
    );
    assert_eq!(h.active(), Some(3));
}

#[test]
fn dead_slot_quarantined_even_with_safety_copy_present() {
    // No automatic promotion of a stashed copy: a dead slot is quarantined outright (the stash
    // itself is the gate's, behind the fake).
    let mut h = expiring_two();
    script(&h, "2", vec![RefreshOutcome::failed("invalid_grant")]);
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.)), ("3", usage(80.))]);
    assert_eq!(h.state()["quarantine"]["2"]["reason"], "invalid_grant");
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn same_uuid_different_org_is_identity_conflict() {
    let h = expiring_two();
    h.host.lock().accounts.get_mut(&2).unwrap().org = "org-2".into();
    let fresh = rotated("2", 99_999_999_999_000.);
    script(
        &h,
        "2",
        vec![granted(
            &fresh,
            Some(identity("uuid-2", "b@example.com", "org-other")),
        )],
    );
    assert_eq!(freshen(&h, "2"), "identity-conflict");
}

#[test]
fn malformed_token_identity_never_breaks_freshen() {
    // A uuid that isn't one (cswap: a number; here, the empty string the type can still carry)
    // is ignored: the refreshed credential is already persisted.
    let h = expiring_two();
    let fresh = rotated("2", 99_999_999_999_000.);
    let malformed = AccountIdentity {
        uuid: "  ".into(),
        email: None,
        organization_uuid: None,
    };
    script(&h, "2", vec![granted(&fresh, Some(malformed))]);
    assert_eq!(freshen(&h, "2"), "ok");
    assert_eq!(credentials_of(&h, 2), fresh);
    assert!(h.host.lock().backfills.is_empty());
}

#[test]
fn blank_uuid_slot_with_org_conflict_quarantines_not_backfills() {
    // The org conflict is checked before the blank-uuid backfill: a wrong-org credential is
    // evidence the slot holds the wrong account, and its uuid would stick to the slot.
    let h = expiring_two();
    {
        let mut fake = h.host.lock();
        let account = fake.accounts.get_mut(&2).unwrap();
        account.uuid = String::new();
        account.org = "org-A".into();
    }
    let fresh = rotated("2", 99_999_999_999_000.);
    script(
        &h,
        "2",
        vec![granted(
            &fresh,
            Some(identity("uuid-real", "z@example.com", "org-B")),
        )],
    );
    assert_eq!(freshen(&h, "2"), "identity-conflict");
    assert_eq!(h.host.lock().accounts[&2].uuid, "");
    assert_eq!(credentials_of(&h, 2), fresh);
}

// -- TestFreshenRoutesThroughGate -------------------------------------------------------------

fn gate_answers(error: &str) -> &'static str {
    let h = Harness::new(defaults());
    h.seed_expiring(2, "b@example.com", Some(1.));
    script(&h, "2", vec![RefreshOutcome::failed(error)]);
    freshen(&h, "2")
}

#[test]
fn lock_contention_is_not_reported_as_network_trouble() {
    // Waiting on another gate is local: "(network?)" would send the person to check a
    // connection that is fine.
    assert_eq!(gate_answers("consume-busy"), "consume-busy");
}

#[test]
fn invalid_client_is_not_reported_as_network_trouble() {
    // A rejected OAuth client is systemic and deterministic, nothing about the slot.
    assert_eq!(gate_answers("invalid_client"), "invalid_client");
}

#[test]
fn unreadable_stash_is_not_reported_as_network_trouble() {
    assert_eq!(gate_answers("stash-unreadable"), "stash-unreadable");
}

#[test]
fn store_unmirrored_keeps_its_own_kind() {
    assert_eq!(gate_answers("store-unmirrored"), "store-unmirrored");
}

#[test]
fn an_actionable_cause_is_not_hidden_by_a_self_clearing_one() {
    // One systemic cause is reported, the actionable one: slot 2 needs a person (a variable is
    // set), slot 3 clears itself, and 3 is tried last.
    let mut h = Harness::new(defaults());
    h.seed(1, "a@example.com");
    h.seed_expiring(2, "b@example.com", Some(1.));
    h.seed_expiring(3, "c@example.com", Some(1.));
    h.make_live(1);
    script(&h, "2", vec![RefreshOutcome::failed("store-unmirrored")]);
    script(&h, "3", vec![RefreshOutcome::failed("consume-busy")]);
    h.tick_with_usage(&[
        ("1", usage7(95., 95., Some(R_LATER))), // active, over the threshold
        ("2", usage7(10., 10., Some(R_SOON))),
        ("3", usage7(10., 10., Some(R_LATEST))),
    ]);
    assert_eq!(h.host.lock().grant_log, ["2", "3"]);
    let message = h
        .all::<ErrorEvent>()
        .last()
        .expect("an error event")
        .message
        .clone();
    assert!(
        message.contains("CLAUDE_SECURESTORAGE_CONFIG_DIR"),
        "got {message:?}"
    );
}

#[test]
fn the_systemic_kinds_are_the_gates_deterministic_errors() {
    // Every deterministic refusal the gate names has its remedy, each its own, the most
    // actionable first.
    let systemic: BTreeSet<Systemic> = DETERMINISTIC_REFRESH_ERRORS
        .into_iter()
        .map(|kind| Systemic::from_kind(kind).expect(kind))
        .collect();
    assert_eq!(systemic.len(), DETERMINISTIC_REFRESH_ERRORS.len());
    assert_eq!(Systemic::from_kind("transient"), None);
    assert!(Systemic::StoreUnmirrored < Systemic::ConsumeBusy);
}

#[test]
fn a_real_transient_still_reads_transient() {
    assert_eq!(gate_answers("transient"), "transient");
}

#[test]
fn freshen_calls_consume_gate() {
    let h = Harness::new(defaults());
    h.seed_expiring(2, "b@example.com", Some(1.)); // near expiry
    let fresh = rotated("2", 9_999_999_999_000.);
    script(&h, "2", vec![granted(&fresh, None)]);
    assert_eq!(freshen(&h, "2"), "ok");
    // The engine has no POST of its own: the gate is the only way to a refresh.
    assert_eq!(h.host.lock().grant_log, ["2"]);
    assert!(h.sandbox.http.requests().is_empty());
}

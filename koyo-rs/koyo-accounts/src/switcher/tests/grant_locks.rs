//! cswap's `tests/test_switcher.py` (0fe22a8) for the refresh gate's locks:
//! `TestConsumeGateLockFailures`, and `TestGateUltraReviewFixes`'s consume-lock cases. cswap
//! patches `FileLock` to fail or watch; here the lock is held for real, by the test or by the
//! token endpoint while the POST is in flight, and the gate's wait for it is short.
//!
//! Adapted: `test_no_store_read_happens_before_the_consume_lock` watches the order of a free
//! lock and the first read; here, with the lock held elsewhere, the gate asks the Keychain
//! nothing. `test_the_consume_lock_is_released_even_when_the_post_raises`: Koyo's POST can't
//! raise, so a transport failure is the closest, and must free the lock too.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::Harness;
use super::grant::racing;
use super::support::{EMAIL, gate_harness, old, posted, queue_grant, tokens};
use crate::http::HttpError;
use crate::locking::FileLock;
use crate::oauth::OAUTH_TOKEN_URL;

/// Koyo's `.lock` held by someone else, and the gate's wait for it short.
fn hold_slot_lock(h: &mut Harness) -> FileLock {
    h.switcher.lock_timeout = Duration::from_millis(100);
    FileLock::hold(&h.sandbox.env(), &h.switcher.lock_file, Duration::ZERO).unwrap()
}

/// Slot 1's consume lock held by another gate, and the wait for it short.
fn hold_consume_lock(h: &mut Harness) -> FileLock {
    h.switcher.lock_timeout = Duration::from_millis(100);
    let path = h.switcher.credentials_dir.join(".consume-1.lock");
    FileLock::hold(&h.sandbox.env(), &path, Duration::ZERO).unwrap()
}

#[test]
fn lock_failure_before_post_is_transient() {
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let _held = hold_slot_lock(&mut h);
    queue_grant(&h, "sk-new", "rt-new");
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert!(posted(&h).is_empty(), "nothing consumed");
    assert_eq!(out.error.as_deref(), Some("transient"));
}

/// The gate with a token endpoint that takes Koyo's `.lock` while the POST is in flight and
/// keeps it, so the persist window can't.
fn lock_taken_during_post(h: &mut Harness) -> Arc<Mutex<Option<FileLock>>> {
    let held = Arc::new(Mutex::new(None));
    let env = h.sandbox.env();
    let lock_file = h.switcher.lock_file.clone();
    let keep = held.clone();
    racing(h, "sk-new", "rt-new", move || {
        *keep.lock().unwrap() = Some(FileLock::hold(&env, &lock_file, Duration::ZERO).unwrap());
    });
    // The rebuilt switcher starts from the default wait.
    h.switcher.lock_timeout = Duration::from_millis(100);
    held
}

#[test]
fn lock_failure_after_post_stashes_successor() {
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let _held = lock_taken_during_post(&mut h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    // The successor survives: returned to the caller AND stashed.
    assert_eq!(
        tokens(&out.credentials.unwrap()),
        ("sk-new".into(), "rt-new".into())
    );
    assert!(!h.stash().is_empty(), "successor must be stashed");
}

#[test]
fn a_stashed_successor_is_not_reported_as_freshened() {
    // The stash leaves the spent grant in the store: activating it would log the account out.
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let _held = lock_taken_during_post(&mut h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(h.backup("1", EMAIL).unwrap(), old());
    assert!(
        out.error.is_some(),
        "a stashed successor reads as freshened"
    );
    assert!(out.stashed);
    assert!(!h.stash().is_empty(), "successor must still be stashed");
}

#[test]
fn no_store_read_happens_before_the_consume_lock() {
    // With the consume lock held elsewhere, the gate asks the Keychain nothing: no read comes
    // before the lock, so two gates can't select the same one-time-use grant.
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let _held = hold_consume_lock(&mut h);
    h.sandbox.security.clear_calls();
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(out.error.as_deref(), Some("consume-busy"));
    assert!(
        h.sandbox.security.calls().is_empty(),
        "a store read came before the lock"
    );
}

#[test]
fn contention_reports_its_own_kind_not_transient() {
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let _held = hold_consume_lock(&mut h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(out.error.as_deref(), Some("consume-busy"));
    assert!(posted(&h).is_empty());
}

#[test]
fn the_consume_lock_is_released_even_when_the_post_raises() {
    // Koyo's POST can't raise; a transport failure is the closest, and must free the lock.
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    h.sandbox
        .http
        .queue(OAUTH_TOKEN_URL, Err(HttpError::Network("boom".into())));
    let first = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(first.error.as_deref(), Some("transient"));
    queue_grant(&h, "sk-new", "rt-new");
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_ne!(out.error.as_deref(), Some("consume-busy"));
    assert_eq!(out.error, None);
}

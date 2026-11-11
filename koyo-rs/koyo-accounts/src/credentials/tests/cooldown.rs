//! The capability cache and its cooldown: `test_switcher.py`'s `TestMacosKeychainFallback`
//! capability cases, and `test_macos_keychain_contract.py`'s
//! `test_a_lapsed_cooldown_clears_the_unreadable_verdict`.

use std::time::{Duration, Instant};

use super::World;
use crate::AccountsError;
use crate::keychain::{self, KeychainError};
use crate::paths;
use crate::testing::Answer;

fn locked_read(world: &World) -> Result<Option<String>, KeychainError> {
    world.security.lock();
    let read = world
        .store
        .kc_call(|security| keychain::get_password(security, "svc", "acct"));
    world.security.clear_rule();
    read
}

#[test]
fn capability_cache_sticky_false() {
    let world = World::new();
    assert!(world.store.use_keychain(), "optimistic before any op");
    assert!(locked_read(&world).is_err());
    assert!(!world.store.use_keychain());
    // A later success doesn't send operations back within the cooldown: no split.
    world.security.fake.set("svc", "acct", "ok");
    world
        .store
        .kc_call(|security| keychain::get_password(security, "svc", "acct"))
        .unwrap();
    assert!(!world.store.use_keychain());
}

#[test]
fn kc_call_failure_schedules_a_recheck() {
    let world = World::new();
    let before = Instant::now();
    assert!(locked_read(&world).is_err());
    assert!(
        world
            .store
            .state()
            .disabled_until
            .is_some_and(|deadline| deadline > before)
    );
}

#[test]
fn keychain_recovers_after_cooldown() {
    // A long-running app re-probes after the cooldown, so a transient timeout doesn't
    // disable the Keychain for the whole process.
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(false));
    world.store.lapse_keychain_cooldown();
    assert!(world.store.use_keychain(), "re-probes");
    let state = world.store.state();
    assert_eq!(state.usable, None, "re-armed for a fresh op");
    assert_eq!(state.disabled_until, None);
}

#[test]
fn keychain_stays_file_mode_during_cooldown() {
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(false));
    world.store.state().disabled_until = Some(Instant::now() + Duration::from_secs(100));
    assert!(!world.store.use_keychain());
}

#[test]
fn item_exists_is_capability_neutral() {
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(false));
    world.security.fake.set("svc", "acct", "x");
    // Not a `kc_call`: its `true` can't send operations back to the Keychain.
    assert!(keychain::item_exists(
        &*world.store.env().security,
        "svc",
        "acct"
    ));
    assert!(!world.store.use_keychain());
}

#[test]
fn capability_cache_is_process_local() {
    let first = World::new();
    first.store.set_keychain_usable_cache(Some(false));
    assert!(!first.store.use_keychain());
    let second = World::new();
    assert_eq!(second.store.state().usable, None);
    assert!(second.store.use_keychain());
}

#[test]
fn a_lapsed_cooldown_clears_the_unreadable_verdict() {
    let world = World::new();
    assert!(locked_read(&world).is_err());
    assert!(world.store.keychain_unreadable(), "inside the cooldown");
    world.store.lapse_keychain_cooldown();
    assert!(
        !world.store.keychain_unreadable(),
        "cooldown lapsed: re-probe"
    );
}

/// cswap's `test_write_fallback_clears_pending_read_reprobe`, with no file to fall back to:
/// inside a read's cooldown the active write fails without spawning `security` and writes no
/// file, and the pending re-probe stays.
#[test]
fn a_write_inside_the_cooldown_fails_and_writes_no_file() {
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(false));
    let deadline = Instant::now() + Duration::from_secs(100);
    world.store.state().disabled_until = Some(deadline);

    let written = world
        .store
        .write_oauth_credentials(r#"{"claudeAiOauth": {"accessToken": "x"}}"#);

    assert!(
        matches!(written, Err(AccountsError::CredentialWrite(_))),
        "{written:?}"
    );
    assert!(world.security.calls().is_empty());
    assert!(!paths::credentials_path(world.store.env()).exists());
    assert_eq!(world.store.state().disabled_until, Some(deadline));
    assert!(!world.store.use_keychain());
}

/// A timeout counts as a failure, as cswap's `KEYCHAIN_ERRORS` do.
#[test]
fn a_timeout_starts_the_cooldown_too() {
    let world = World::new();
    world.security.fake.answer_next(Answer::Timeout);
    assert!(
        world
            .store
            .kc_call(|security| keychain::get_password(security, "svc", "acct"))
            .is_err()
    );
    assert!(!world.store.use_keychain());
    assert!(world.store.keychain_unreadable());
}

//! The backup cases of `test_switcher.py`: `TestMacosKeychainFallback`'s Keychain halves and
//! `TestStashAndRetentionStore`'s retention cases. cswap runs the retention ones on Linux,
//! against `.enc` files; here they run against the Keychain, where Koyo keeps backups.

use log::Level;

use super::{World, logs, verb};
use crate::AccountsError;
use crate::paths::BACKUP_KEYCHAIN_SERVICE;
use crate::testing::Answer;

fn has(world: &World, account: &str) -> bool {
    world
        .security
        .fake
        .get(BACKUP_KEYCHAIN_SERVICE, account)
        .is_some()
}

/// cswap's `test_backup_file_mode_writes_enc_and_clears_keychain`, with no file mode: a
/// refused backup write fails, and the Keychain copy, the only one, stays.
#[test]
fn backup_write_failure_fails_and_keeps_the_keychain_copy() {
    let world = World::new();
    world
        .store
        .kc_write_backup("1", "a@example.com", "STALE-KC")
        .unwrap();
    world
        .security
        .set_rule(|call| (verb(call) == "add-generic-password").then_some(Answer::Exit(51)));
    let written = world
        .store
        .write_account_credentials("1", "a@example.com", "FILE-CREDS");
    assert!(
        matches!(written, Err(AccountsError::CredentialWrite(_))),
        "{written:?}"
    );
    world.security.clear_rule();
    world.store.lapse_keychain_cooldown();
    assert_eq!(
        world
            .store
            .read_account_credentials("1", "a@example.com", false),
        "STALE-KC"
    );
    assert!(!world.store.credentials_dir().exists(), "no .enc written");
}

#[test]
fn backup_delete_removes_both_backends() {
    // The Keychain half; Koyo keeps no `.enc`.
    let world = World::new();
    world
        .store
        .kc_write_backup("1", "a@example.com", "KC")
        .unwrap();
    world.store.delete_account_credentials("1", "a@example.com");
    assert!(!has(&world, "account-1-a@example.com"));
}

#[test]
fn prev_keychain_item_retained_readable_control() {
    let world = World::new();
    world
        .store
        .kc_write_backup("1", "a@example.com", "gen-1")
        .unwrap();
    world
        .store
        .write_account_credentials("1", "a@example.com", "gen-2")
        .unwrap();
    assert!(has(&world, "account-1-a@example.com.prev"));
    assert_eq!(
        world.store.read_previous_backup("1", "a@example.com"),
        "gen-1"
    );
}

#[test]
fn prev_generation_retained_on_overwrite() {
    let world = World::new();
    let store = &world.store;
    store
        .write_account_credentials("1", "a@b.c", "gen-1")
        .unwrap();
    store
        .write_account_credentials("1", "a@b.c", "gen-2")
        .unwrap();
    assert_eq!(store.read_account_credentials("1", "a@b.c", false), "gen-2");
    assert_eq!(store.read_previous_backup("1", "a@b.c"), "gen-1");
    // Rewriting the same value doesn't clobber the retained generation.
    store
        .write_account_credentials("1", "a@b.c", "gen-2")
        .unwrap();
    assert_eq!(store.read_previous_backup("1", "a@b.c"), "gen-1");
}

#[test]
fn prev_removed_with_account() {
    let world = World::new();
    let store = &world.store;
    store
        .write_account_credentials("1", "a@b.c", "gen-1")
        .unwrap();
    store
        .write_account_credentials("1", "a@b.c", "gen-2")
        .unwrap();
    store.delete_account_credentials("1", "a@b.c");
    assert_eq!(store.read_previous_backup("1", "a@b.c"), "");
}

#[test]
fn unreadable_current_backup_is_logged_distinguishably_at_info() {
    // The current backup can't be read (a locked Keychain here, an unreadable `.enc` in
    // cswap's): that's said at INFO, and the read failure itself once at WARNING.
    let world = World::new();
    world
        .store
        .write_account_credentials("1", "a@b.c", "gen-1")
        .unwrap();
    world.security.lock();
    logs::capture();
    world.store.retain_previous_backup("1", "a@b.c", "gen-2");
    let retention = logs::containing("Could not retain previous credential generation");
    assert!(!retention.is_empty(), "nothing told unreadable from absent");
    assert!(retention.iter().all(|(level, _)| *level == Level::Info));
    assert!(
        logs::containing("Failed to read credentials from Keychain")
            .iter()
            .any(|(level, _)| *level == Level::Warn),
        "the read failure itself is reported once at WARNING"
    );
    assert!(
        !has(&world, "account-1-a@b.c.prev"),
        "no .prev for this write"
    );
}

#[test]
fn a_backup_write_inside_the_cooldown_fails_without_spawning_a_write() {
    let world = World::new();
    world.security.lock();
    world.store.read_account_credentials("1", "a@b.c", false);
    world.security.clear_rule();
    world.security.clear_calls();
    let written = world.store.write_account_credentials("1", "a@b.c", "fresh");
    assert!(
        matches!(written, Err(AccountsError::CredentialWrite(_))),
        "{written:?}"
    );
    assert!(
        !world
            .security
            .calls()
            .iter()
            .any(|call| verb(call) == "add-generic-password")
    );
    assert!(!world.store.credentials_dir().exists());
}

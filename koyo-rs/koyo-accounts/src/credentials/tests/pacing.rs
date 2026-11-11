//! `test_backup_read_pacing.py`: backup Keychain reads are paced by the failure cooldown and
//! the display read cache (#496). cswap's switcher fixture becomes `n` Keychain-only backups,
//! the live item and a `sequence.json`; its snapshot pass becomes the reads a snapshot makes.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use log::Level;

use super::{World, cred, logs, verb};
use crate::credentials::BackupReadError;
use crate::fsutil;
use crate::paths::{self, BACKUP_KEYCHAIN_SERVICE, CLAUDE_CODE_KEYCHAIN_SERVICE};
use crate::testing::Answer;

fn email(num: usize) -> String {
    format!("user{num}@example.com")
}

fn account(num: usize) -> String {
    format!("account-{num}-{}", email(num))
}

fn registry(updated: &str, n: usize) -> String {
    serde_json::json!({
        "activeAccountNumber": 1,
        "lastUpdated": updated,
        "sequence": (1..=n).collect::<Vec<_>>(),
    })
    .to_string()
}

/// `n` accounts, slot 1 active, Keychain-only backups.
fn world(n: usize) -> World {
    let world = World::new();
    let sequence = paths::store::sequence_file(world.store.env());
    fs::write(&sequence, registry("2024-01-01T00:00:00Z", n)).unwrap();
    for num in 1..=n {
        world.security.fake.set(
            BACKUP_KEYCHAIN_SERVICE,
            &account(num),
            &cred(&format!("backup{num}")),
        );
    }
    world.security.fake.set(
        CLAUDE_CODE_KEYCHAIN_SERVICE,
        &world.account(),
        &cred("live1"),
    );
    world
}

fn read(world: &World, num: usize, cached: bool) -> String {
    world
        .store
        .read_account_credentials(&num.to_string(), &email(num), cached)
}

fn read_ex(world: &World, num: usize, cached: bool) -> (String, bool) {
    world
        .store
        .read_account_credentials_ex(&num.to_string(), &email(num), cached)
}

fn missing() -> (String, bool) {
    (String::new(), true)
}

#[test]
fn a_failed_backup_read_backs_off_the_next_one() {
    let world = world(2);
    world.security.lock();
    assert_eq!(read_ex(&world, 2, false), missing());
    assert_eq!(world.security.reads_of(&account(2)), 1);

    world.security.clear_rule(); // the Keychain recovers at once...
    assert_eq!(read_ex(&world, 2, false), missing());
    assert_eq!(
        world.security.reads_of(&account(2)),
        1,
        "a read inside the cooldown spawned"
    );

    world.store.lapse_keychain_cooldown(); // ...the cooldown lapses
    assert_eq!(read_ex(&world, 2, false), (cred("backup2"), false));
    assert_eq!(world.security.reads_of(&account(2)), 2);
}

#[test]
fn the_active_reads_failure_backs_off_backup_reads_too() {
    let world = world(2);
    world.security.set_rule(|call| {
        (super::after(call, "-s").as_deref() == Some(CLAUDE_CODE_KEYCHAIN_SERVICE))
            .then_some(Answer::Exit(51))
    });
    assert!(world.store.read_active_credentials().keychain_unavailable);
    world.security.clear_rule();
    assert_eq!(read_ex(&world, 2, false), missing());
    assert_eq!(world.security.reads_of(&account(2)), 0);
}

/// The snapshot pass of cswap's test, as the reads it makes: the active read, then each idle
/// slot's cached backup read.
fn snapshot(world: &World, n: usize) -> Vec<(String, bool)> {
    world.store.read_active_credentials();
    (2..=n).map(|num| read_ex(world, num, true)).collect()
}

#[test]
fn a_cooldown_is_announced_once_and_its_skips_are_silent() {
    let world = world(5);
    world.security.lock();
    logs::capture();
    let mut idle = Vec::new();
    for _ in 0..2 {
        idle = snapshot(&world, 5);
    }
    assert_eq!(idle, vec![missing(); 4]);
    let skipped_as_failures: Vec<_> = logs::containing("Failed to read credentials from Keychain")
        .into_iter()
        .filter(|(level, _)| *level <= Level::Warn)
        .collect();
    assert_eq!(
        skipped_as_failures,
        [],
        "cooldown skips were logged as failures"
    );
    assert_eq!(
        logs::containing("Keychain backed off").len(),
        1,
        "one per cooldown"
    );

    // The cooldown lapses and the re-probe fails again: a new cooldown, announced once.
    world.store.lapse_keychain_cooldown();
    snapshot(&world, 5);
    snapshot(&world, 5);
    assert_eq!(logs::containing("Keychain backed off").len(), 2);
}

#[test]
fn cached_reads_spawn_once_per_slot_until_the_ttl() {
    let world = world(2);
    for _ in 0..3 {
        assert_eq!(read(&world, 2, true), cred("backup2"));
    }
    assert_eq!(world.security.reads_of(&account(2)), 1);

    world.store.set_backup_read_cache_ttl(Duration::ZERO);
    assert_eq!(read(&world, 2, true), cred("backup2"));
    assert_eq!(
        world.security.reads_of(&account(2)),
        2,
        "an expired entry is re-read"
    );
}

#[test]
fn the_default_read_always_spawns() {
    let world = world(2);
    read(&world, 2, true);
    read(&world, 2, false);
    read_ex(&world, 2, false);
    assert_eq!(world.security.reads_of(&account(2)), 3);
    read(&world, 2, true);
    assert_eq!(world.security.reads_of(&account(2)), 3);
}

#[test]
fn own_write_and_delete_drop_the_cache() {
    let world = world(2);
    assert_eq!(read(&world, 2, true), cred("backup2"));
    world
        .store
        .write_account_credentials("2", &email(2), &cred("rotated"))
        .unwrap();
    let before = world.security.reads_of(&account(2));
    assert_eq!(
        read(&world, 2, true),
        cred("rotated"),
        "served the superseded generation"
    );
    assert_eq!(world.security.reads_of(&account(2)), before + 1);
    assert_eq!(read(&world, 2, true), cred("rotated"));
    assert_eq!(world.security.reads_of(&account(2)), before + 1);

    world.store.delete_account_credentials("2", &email(2));
    let before = world.security.reads_of(&account(2));
    assert_eq!(read(&world, 2, true), "");
    assert_eq!(world.security.reads_of(&account(2)), before + 1);
}

#[test]
fn a_registry_change_drops_the_cache() {
    // Same length, same mtime: the atomic write's new inode still tells them apart.
    let world = world(2);
    read(&world, 2, true);
    read(&world, 2, true);
    assert_eq!(world.security.reads_of(&account(2)), 1);

    let sequence = paths::store::sequence_file(world.store.env());
    let mtime = fs::metadata(&sequence).unwrap().modified().unwrap();
    let rewritten = registry("2024-06-01T00:00:00Z", 2);
    fsutil::atomic_write(world.store.env(), &sequence, rewritten.as_bytes()).unwrap();
    fs::File::options()
        .write(true)
        .open(&sequence)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    read(&world, 2, true);
    assert_eq!(world.security.reads_of(&account(2)), 2);
}

#[test]
fn a_failed_read_is_never_cached() {
    let world = world(2);
    world.security.lock();
    assert_eq!(read_ex(&world, 2, true), missing());
    world.security.clear_rule();
    world.store.lapse_keychain_cooldown();
    assert_eq!(
        read_ex(&world, 2, true),
        (cred("backup2"), false),
        "the failure was served from the cache as an absent backup"
    );
    assert_eq!(world.security.reads_of(&account(2)), 2);
}

#[test]
fn a_cached_hit_is_served_during_the_cooldown() {
    // A transient failure mustn't blank every idle row; a cold slot reads unreadable.
    let world = world(3);
    assert_eq!(read(&world, 2, true), cred("backup2"));
    world.security.lock();
    assert!(matches!(
        world.store.kc_read_backup("9", "nobody@example.com", false),
        Err(BackupReadError::Keychain(_))
    ));
    world.security.clear_rule();

    assert_eq!(read_ex(&world, 2, true), (cred("backup2"), false));
    assert_eq!(world.security.reads_of(&account(2)), 1);
    assert_eq!(read_ex(&world, 3, true), missing());
    assert_eq!(world.security.reads_of(&account(3)), 0);
}

#[test]
fn a_drop_during_the_fill_wins() {
    let world = world(2);
    let store = Arc::downgrade(&world.store);
    world.security.set_rule(move |call| {
        if super::after(call, "-a") == Some(account(2))
            && let Some(store) = store.upgrade()
        {
            store.drop_backup_read_cache(); // lands while `security` runs
        }
        None
    });
    assert_eq!(read(&world, 2, true), cred("backup2"));
    world.security.clear_rule();
    read(&world, 2, true);
    assert_eq!(
        world.security.reads_of(&account(2)),
        2,
        "a fill that raced a drop was kept"
    );
}

#[test]
fn a_write_landing_after_a_racing_fill_is_not_shadowed() {
    // Drop, a miss reads the OLD item and stores it, the write lands: the drop runs after.
    let world = world(2);
    let store = Arc::downgrade(&world.store);
    world.security.set_rule(move |call| {
        if verb(call) == "add-generic-password"
            && super::after(call, "-a") == Some(account(2))
            && let Some(store) = store.upgrade()
        {
            assert_eq!(
                store.read_account_credentials("2", &email(2), true),
                cred("backup2")
            );
        }
        None
    });
    world
        .store
        .kc_write_backup("2", &email(2), &cred("rotated"))
        .unwrap();
    world.security.clear_rule();
    assert_eq!(
        read(&world, 2, true),
        cred("rotated"),
        "the cache shadowed a write that landed after a racing fill"
    );
}

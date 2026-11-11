//! `test_macos_keychain_contract.py`: the backup path's `(service, account)` shape, and the
//! Keychain verdicts of `TestOurOwnFileModeIsNotAKeychainFailure` that don't rest on the
//! file-mode pin. Where cswap's write falls back to a file and pins, Koyo's write fails; the
//! verdict the test is about is asserted across that failed write.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::{World, after, verb};
use crate::fsutil;
use crate::paths::BACKUP_KEYCHAIN_SERVICE;
use crate::testing::Answer;

/// The `(verb, service, account)` of each call.
fn shapes(world: &World) -> Vec<(String, String, String)> {
    world
        .security
        .calls()
        .iter()
        .map(|call| {
            (
                verb(call),
                after(call, "-s").unwrap_or_default(),
                after(call, "-a").unwrap_or_default(),
            )
        })
        .collect()
}

fn shape(verb: &str, account: &str) -> (String, String, String) {
    (verb.into(), BACKUP_KEYCHAIN_SERVICE.into(), account.into())
}

#[test]
fn read_account_credentials_uses_security_service() {
    let world = World::new();
    world.security.fake.set(
        BACKUP_KEYCHAIN_SERVICE,
        "account-1-user@example.com",
        "fake-token",
    );
    let result = world
        .store
        .read_account_credentials("1", "user@example.com", false);
    assert_eq!(result, "fake-token");
    assert_eq!(
        shapes(&world),
        [shape("find-generic-password", "account-1-user@example.com")]
    );
}

#[test]
fn write_account_credentials_uses_security_service() {
    // No backup yet: nothing to retain, so the write is one Keychain write.
    let world = World::new();
    world
        .store
        .write_account_credentials("2", "alice@example.com", "secret-token")
        .unwrap();
    let writes: Vec<_> = shapes(&world)
        .into_iter()
        .filter(|(verb, _, _)| verb == "add-generic-password")
        .collect();
    assert_eq!(
        writes,
        [shape("add-generic-password", "account-2-alice@example.com")]
    );
    assert_eq!(
        world
            .security
            .fake
            .get(BACKUP_KEYCHAIN_SERVICE, "account-2-alice@example.com")
            .as_deref(),
        Some("secret-token")
    );
    world.assert_no_secret_in_calls("secret-token");
}

#[test]
fn write_retains_prev_generation_in_keychain_not_a_file() {
    let world = World::new();
    world.security.fake.set(
        BACKUP_KEYCHAIN_SERVICE,
        "account-2-alice@example.com",
        "old-generation",
    );
    world
        .store
        .write_account_credentials("2", "alice@example.com", "secret-token")
        .unwrap();
    let writes: Vec<_> = shapes(&world)
        .into_iter()
        .filter(|(verb, _, _)| verb == "add-generic-password")
        .collect();
    assert_eq!(
        writes,
        [
            shape("add-generic-password", "account-2-alice@example.com.prev"),
            shape("add-generic-password", "account-2-alice@example.com"),
        ]
    );
    let fake = &world.security.fake;
    assert_eq!(
        fake.get(BACKUP_KEYCHAIN_SERVICE, "account-2-alice@example.com.prev")
            .as_deref(),
        Some("old-generation")
    );
    assert!(
        !world.store.credentials_dir().exists(),
        "no file for the retained generation"
    );
    world.assert_no_secret_in_calls("old-generation");
    world.assert_no_secret_in_calls("secret-token");
}

#[test]
fn delete_account_credentials_uses_security_service() {
    let world = World::new();
    world
        .store
        .delete_account_credentials("3", "bob@example.com");
    assert_eq!(
        shapes(&world),
        [
            shape("delete-generic-password", "account-3-bob@example.com"),
            shape("delete-generic-password", "account-3-bob@example.com.prev"),
            shape("delete-generic-password", "account-None-bob@example.com"),
            shape(
                "delete-generic-password",
                "account-None-bob@example.com.prev"
            ),
        ]
    );
}

#[test]
fn a_write_fallback_does_not_certify_an_unread_backup() {
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(true));
    world.security.lock();
    assert_eq!(
        world
            .store
            .read_account_credentials_ex("3", "c@e.com", false),
        (String::new(), true),
        "premise: a locked Keychain makes an empty backup read unprovable"
    );
    // The active write that cswap falls back and pins on fails here.
    assert!(
        world
            .store
            .write_oauth_credentials(r#"{"claudeAiOauth": {"accessToken": "sk-fb"}}"#)
            .is_err()
    );
    let (_, unreadable) = world
        .store
        .read_account_credentials_ex("3", "c@e.com", false);
    assert!(
        unreadable,
        "a failed active write doesn't certify the backup empty"
    );
}

#[test]
fn a_write_fallback_does_not_certify_the_active_read_either() {
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(true));
    world.security.lock();
    assert!(world.store.read_active_credentials().degraded, "premise");
    assert!(
        world
            .store
            .write_oauth_credentials(r#"{"claudeAiOauth": {"accessToken": "x"}}"#)
            .is_err()
    );
    assert!(
        world.store.read_active_credentials().degraded,
        "the Keychain is still locked; the slot isn't proven empty"
    );
}

#[test]
fn a_real_keychain_failure_still_reports_unreadable() {
    let world = World::new();
    world
        .security
        .set_rule(|call| (verb(call) == "find-generic-password").then_some(Answer::Exit(51)));
    let (_, unreadable) = world
        .store
        .read_account_credentials_ex("9", "x@e.com", false);
    assert!(unreadable);
}

#[test]
fn a_stale_failure_flag_does_not_condemn_a_working_read() {
    // An earlier op failed; the Keychain answers now that slot 9 has no backup.
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(false));
    let (_, unreadable) = world
        .store
        .read_account_credentials_ex("9", "x@e.com", false);
    assert!(!unreadable);
}

#[test]
fn a_recovered_keychain_is_not_condemned_by_the_previous_read() {
    // About the per-read verdict, not the backoff: the deadline lapses at once.
    let world = World::new();
    world.store.set_keychain_cooldown(Duration::ZERO);
    world.security.lock();
    assert_eq!(
        world
            .store
            .read_account_credentials_ex("9", "x@e.com", false),
        (String::new(), true)
    );
    world.security.clear_rule(); // recovered
    let (_, unreadable) = world
        .store
        .read_account_credentials_ex("9", "x@e.com", false);
    assert!(!unreadable, "the previous read's failure outlived it");
}

#[test]
fn an_idle_backup_read_does_not_erase_the_active_verdict() {
    // A success on one item is no evidence about another.
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(true));
    world.security.set_rule(|call| {
        after(call, "-s")
            .is_some_and(|service| service.contains("credentials"))
            .then_some(Answer::Exit(51))
    });
    assert!(world.store.read_active_credentials().degraded, "premise");
    world
        .store
        .read_account_credentials("3", "c@example.com", false);
    assert!(
        world.store.read_active_credentials().degraded,
        "an unrelated slot's readable backup erased the active read's own failure"
    );
}

/// The same rule where the cooldown doesn't hide it: in Koyo the backup read above is backed
/// off, so it never reaches the Keychain. A `.prev` read isn't gated by the cooldown, and its
/// success clears the process's failure flag; the active read's own verdict must survive it.
#[test]
fn an_ungated_success_inside_the_cooldown_does_not_erase_the_active_verdict() {
    let world = World::new();
    world.security.set_rule(|call| {
        after(call, "-s")
            .is_some_and(|service| service.contains("credentials"))
            .then_some(Answer::Exit(51))
    });
    assert!(world.store.read_active_credentials().degraded, "premise");
    assert_eq!(world.store.read_previous_backup("3", "c@example.com"), "");
    assert!(
        !world.store.state().op_failed,
        "premise: a success cleared the flag"
    );
    assert!(world.store.read_active_credentials().degraded);
}

#[test]
fn two_concurrent_backup_reads_keep_their_own_verdicts() {
    // Slot 2 genuinely denied while a sibling loop reads an empty slot 9, each `security`
    // taking 4 ms; without the cooldown the race runs on every read.
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(true));
    world.store.set_keychain_cooldown(Duration::ZERO);
    world.security.set_rule(|call| {
        fsutil::nap(Duration::from_millis(4));
        if after(call, "-a").is_some_and(|account| account.contains("account-2-")) {
            return Some(Answer::Exit(51));
        }
        Some(super::ABSENT)
    });
    let stop = Arc::new(AtomicBool::new(false));
    let sibling = {
        let (store, stop) = (world.store.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                store.read_account_credentials_ex("9", "i@e.com", false);
            }
        })
    };
    let lost = (0..60)
        .filter(|_| {
            !world
                .store
                .read_account_credentials_ex("2", "b@e.com", false)
                .1
        })
        .count();
    stop.store(true, Ordering::Relaxed);
    sibling.join().unwrap();
    assert_eq!(
        lost, 0,
        "reads lost their own unreadable verdict to a sibling read"
    );
}

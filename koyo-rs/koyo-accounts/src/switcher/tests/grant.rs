//! cswap's `tests/test_switcher.py` (0fe22a8) for the refresh gate, `consume_backup_grant`:
//! `TestConsumeGate`, `TestConsumeGateLockFailures`, `TestPermanentlyUnreadableStashRow`,
//! `TestStoreResolutionParity`'s consume cases, and the gate cases of
//! `TestGateUltraReviewFixes` and `TestUltraReviewCoverageGaps`. The lock cases are in
//! `grant_locks.rs`, the stash cases in `grant_stash.rs`, the faults and the race in
//! `grant_faults.rs`.
//!
//! Skipped, with session mode: `test_gate_prefers_newer_session_profile_lineage`,
//! `test_an_absent_slot_defers_even_when_a_newer_profile_exists` (its absent-slot half is
//! `a_removed_slot_defers_instead_of_posting_the_snapshot`),
//! `test_stale_marked_profile_never_supersedes_backup`,
//! `test_a_foreign_profile_never_supersedes_the_backup`,
//! `test_an_older_profile_never_supersedes_the_backup`,
//! `test_session_shell_config_dir_refuses_switch`. The `ERROR_NOTES` half of
//! `test_the_kind_carries_its_remedy_and_skips_the_doomed_fetch` is printing, not ported.

use std::sync::Arc;

use serde_json::json;

use super::support::{
    EMAIL, creds, gate_harness, grant, new, old, posted, queue_grant, queue_invalid_grant,
    stash_successor, tokens,
};
use super::{Harness, ScriptedHttp, sample_sequence_data};
use crate::credentials::{StashReason, backup_username};
use crate::http::HttpError;
use crate::oauth::{DETERMINISTIC_REFRESH_ERRORS, OAUTH_TOKEN_URL, credential_fingerprint};
use crate::paths::BACKUP_KEYCHAIN_SERVICE;
use crate::testing::Answer;

/// A stash row saying a prior gate spent `consumed`'s grant and couldn't persist `successor`.
pub(super) fn stash_successor_of(h: &Harness, successor: &str, consumed: &str) -> String {
    let consumed_fp = credential_fingerprint(consumed);
    stash_successor(
        h,
        successor,
        StashReason::PersistFailed,
        consumed_fp.as_deref(),
    )
}

/// The gate's switcher with a token endpoint that runs `during` while the POST is in flight
/// and then grants `(access, refresh)`.
pub(super) fn racing(
    h: &mut Harness,
    access: &'static str,
    refresh: &'static str,
    during: impl Fn() + Send + Sync + 'static,
) {
    h.rebuild_with_http(Arc::new(ScriptedHttp(
        move |request: &crate::http::Request| {
            if request.url.starts_with(OAUTH_TOKEN_URL) {
                during();
                return Ok(grant(access, refresh));
            }
            Err(HttpError::Network("nothing queued".into()))
        },
    )));
}

#[test]
fn gate_rereads_under_lock_and_posts_rereread_bytes() {
    // The POST spends the bytes re-read under the lock, not the caller's stale snapshot.
    let h = gate_harness();
    let fresher = creds("sk-fresher", "rt-fresher", json!(2000));
    h.set_backup("1", EMAIL, &fresher);
    queue_grant(&h, "sk-new", "rt-new");
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(posted(&h), ["rt-fresher"]);
    let successor = result.credentials.unwrap();
    assert_eq!(tokens(&successor), ("sk-new".into(), "rt-new".into()));
    assert_eq!(h.backup("1", EMAIL).unwrap(), successor);
}

#[test]
fn gate_cas_persist_detects_racing_writer() {
    // A writer replaced the lineage mid-POST: our successor is stashed, theirs adopted.
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let racer = creds("sk-racer", "rt-racer", json!(8_888_888_888_000_u64));
    let security = h.sandbox.security.clone();
    let written = racer.clone();
    racing(&mut h, "sk-new", "rt-new", move || {
        security.set(
            BACKUP_KEYCHAIN_SERVICE,
            &backup_username("1", EMAIL),
            &written,
        );
    });
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(h.backup("1", EMAIL).unwrap(), racer);
    assert_eq!(result.credentials.unwrap(), racer);
    assert!(
        !h.stash().is_empty(),
        "consumed successor must be stashed, never discarded"
    );
}

#[test]
fn a_cas_conflict_is_not_reported_as_a_failed_freshen() {
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let racer = creds("sk-rac", "rt-rac", json!(8_888_888_888_000_u64));
    let security = h.sandbox.security.clone();
    let written = racer.clone();
    racing(&mut h, "sk-new", "rt-new", move || {
        security.set(
            BACKUP_KEYCHAIN_SERVICE,
            &backup_username("1", EMAIL),
            &written,
        );
    });
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(h.backup("1", EMAIL).unwrap(), racer);
    assert_eq!(out.credentials.unwrap(), racer);
    assert_eq!(out.error, None, "a freshened slot reported as a failure");
}

#[test]
fn gate_invalid_grant_returns_error_without_persist() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    queue_invalid_grant(&h);
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(result.error.as_deref(), Some("invalid_grant"));
    assert_eq!(h.backup("1", EMAIL).unwrap(), old(), "no destructive write");
}

#[test]
fn an_unreadable_stash_manifest_defers_instead_of_posting() {
    // A manifest that can't be READ isn't one with no rows: the successor may be in it.
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    stash_successor_of(&h, &new(), &old());
    let manifest = h.switcher.store.stash_manifest_path();
    super::support::chmod(&manifest, 0o000);
    queue_grant(&h, "sk-new", "rt-new");
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    super::support::chmod(&manifest, 0o600);
    assert!(
        posted(&h).is_empty(),
        "POSTed the spent generation while its successor sat in a stash the gate could not see"
    );
    assert_eq!(result.error.as_deref(), Some("stash-unreadable"));
}

#[test]
fn the_purge_exit_the_fail_closed_message_names_actually_works() {
    // Failing closed on corrupt + orphans is defensible only because the exit works.
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    stash_successor_of(&h, &new(), &old());
    std::fs::write(h.switcher.store.stash_manifest_path(), "{not json at all").unwrap();
    let listed = h.stash();
    assert!(!listed.is_empty(), "a corrupt manifest hid the orphan");
    for entry_id in listed.keys() {
        h.switcher
            .store
            .remove_unclaimed_credential(entry_id)
            .unwrap();
    }
    assert!(!h.switcher.store.stash_entry_files_exist());
    queue_grant(&h, "sk-new", "rt-new");
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(
        posted(&h),
        ["rt-old"],
        "the documented exit did not unblock it"
    );
    assert_eq!(result.error, None);
}

#[test]
fn a_corrupt_manifest_with_orphan_entries_fails_closed() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    stash_successor_of(&h, &new(), &old());
    std::fs::write(h.switcher.store.stash_manifest_path(), "{not json at all").unwrap();
    queue_invalid_grant(&h);
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert!(
        posted(&h).is_empty(),
        "POSTed a spent grant with a successor on disk"
    );
    assert_eq!(result.error.as_deref(), Some("stash-unreadable"));
}

#[test]
fn a_corrupt_manifest_still_posts_rather_than_deadlocking() {
    // Corrupt with nothing to protect: proceed, so the next write can set the file aside.
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    h.switcher.setup_directories().unwrap();
    std::fs::write(h.switcher.store.stash_manifest_path(), "{not json at all").unwrap();
    queue_grant(&h, "sk-new", "rt-new");
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(posted(&h), ["rt-old"], "a corrupt manifest froze the slot");
    assert_eq!(result.error, None);
}

#[test]
fn a_manifest_of_invalid_utf8_is_corrupt_not_unreadable() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    stash_successor_of(&h, &new(), &old());
    std::fs::write(
        h.switcher.store.stash_manifest_path(),
        b"\xff\xfe\x00not utf8",
    )
    .unwrap();
    let (entries, verdict) = h.switcher.store.read_stash_manifest_ex();
    assert!(entries.is_empty());
    assert_eq!(verdict.as_str(), "corrupt");
}

#[test]
fn a_readable_empty_manifest_still_posts() {
    // The control: nothing stashed really means nothing to adopt.
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    queue_grant(&h, "sk-new", "rt-new");
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(posted(&h), ["rt-old"]);
    assert_eq!(result.error, None);
}

#[test]
fn a_removed_slot_defers_instead_of_posting_the_snapshot() {
    // No stored credential: a removal landed first. The snapshot must not resurrect it.
    let h = gate_harness();
    queue_grant(&h, "sk-new", "rt-new");
    let result = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert!(
        posted(&h).is_empty(),
        "spent a grant for a slot that no longer exists"
    );
    assert_eq!(result.error.as_deref(), Some("transient"));
    assert!(
        h.stash().is_empty(),
        "stashed a successor for an account the person deleted"
    );
}

#[test]
fn ten_passes_name_the_condition_instead_of_network() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let consumed_fp = credential_fingerprint(&old());
    let entry_id = stash_successor(
        &h,
        &new(),
        StashReason::PersistLockFailed,
        consumed_fp.as_deref(),
    );
    let entry_path = h.switcher.store.stash_entry_path(&entry_id);
    super::support::chmod(&entry_path, 0o000);
    let errors: std::collections::BTreeSet<Option<String>> = (0..10)
        .map(|_| h.switcher.consume_backup_grant("1", EMAIL, &old()).error)
        .collect();
    super::support::chmod(&entry_path, 0o600);
    assert!(
        posted(&h).is_empty(),
        "the spent generation must never be POSTed"
    );
    assert_eq!(
        errors,
        std::collections::BTreeSet::from([Some("stash-unreadable".to_owned())])
    );
    assert!(
        h.stash().contains_key(&entry_id),
        "the row is the sole copy of a generation the slot already consumed"
    );
}

#[test]
fn the_kind_carries_its_remedy_and_skips_the_doomed_fetch() {
    assert!(DETERMINISTIC_REFRESH_ERRORS.contains(&"stash-unreadable"));
}

#[test]
fn failure_outcome_carries_consumed_fp_of_posted_bytes() {
    let h = gate_harness();
    let fresher = creds("sk-fresher", "rt-fresher", json!(2000));
    h.set_backup("1", EMAIL, &fresher);
    queue_invalid_grant(&h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(out.error.as_deref(), Some("invalid_grant"));
    assert_eq!(out.consumed_fp, credential_fingerprint(&fresher));
    assert_ne!(out.consumed_fp, credential_fingerprint(&old()));
}

#[test]
fn next_gate_pass_adopts_stashed_successor() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    stash_successor_of(&h, &new(), &old());
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert!(posted(&h).is_empty());
    assert_eq!(out.error, None);
    assert_eq!(out.credentials.unwrap(), new());
    assert_eq!(h.backup("1", EMAIL).unwrap(), new());
    assert!(h.stash().is_empty(), "stash entry consumed");
}

#[test]
fn slot_removed_mid_post_stashes_instead_of_resurrecting() {
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let security = h.sandbox.security.clone();
    racing(&mut h, "sk-new", "rt-new", move || {
        security.remove(BACKUP_KEYCHAIN_SERVICE, &backup_username("1", EMAIL));
    });
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(out.error, None);
    assert_eq!(h.backup("1", EMAIL), None, "removed slot must stay empty");
    assert!(!h.stash().is_empty(), "successor parked in stash");
}

#[test]
fn unreadable_backup_defers_instead_of_posting_snapshot() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    h.sandbox
        .security
        .answer_service(BACKUP_KEYCHAIN_SERVICE, Answer::Exit(36));
    queue_grant(&h, "sk-new", "rt-new");
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert!(posted(&h).is_empty());
    assert_eq!(out.error.as_deref(), Some("transient"));
}

#[test]
fn fresh_rotated_reread_adopted_without_second_consume() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &new());
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert!(posted(&h).is_empty());
    assert_eq!(out.error, None);
    assert_eq!(out.credentials.unwrap(), new());
}

#[test]
fn gate_success_never_touches_live_store() {
    let h = gate_harness();
    let stored = creds("a", "rt", json!(1000));
    h.set_backup("1", EMAIL, &stored);
    queue_grant(&h, "b", "rt2");
    let out = h.switcher.consume_backup_grant("1", EMAIL, &stored);
    let successor = out.credentials.unwrap();
    assert_eq!(tokens(&successor), ("b".into(), "rt2".into()));
    assert_eq!(h.live(), None, "the gate wrote the live store");
    assert_eq!(h.backup("1", EMAIL).unwrap(), successor);
}

#[test]
fn securestorage_env_refuses_consume() {
    let mut h = Harness::new();
    h.sandbox.vars.securestorage_config_dir = Some("/elsewhere/other".into());
    h.rebuild();
    h.write_sequence(&sample_sequence_data());
    let stored = creds("a", "rt", json!(1000));
    h.set_backup("1", EMAIL, &stored);
    queue_grant(&h, "b", "rt2");
    let result = h.switcher.consume_backup_grant("1", EMAIL, &stored);
    assert!(posted(&h).is_empty());
    assert_eq!(result.error.as_deref(), Some("store-unmirrored"));
}

#[test]
fn normal_env_unaffected() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    let stored = creds("a", "rt", json!(1000));
    h.set_backup("1", EMAIL, &stored);
    queue_grant(&h, "b", "rt2");
    let result = h.switcher.consume_backup_grant("1", EMAIL, &stored);
    assert_eq!(
        tokens(&result.credentials.unwrap()),
        ("b".into(), "rt2".into())
    );
}

//! cswap's `tests/test_switcher.py` (0fe22a8) for the gate's side of the stash: the gate
//! halves of `TestStashReaderUnreadableVsAbsent` (the reader halves are the credential
//! store's, in `credentials/tests/stash.rs`), and `TestGateUltraReviewFixes`'s
//! `_adopt_stashed_successor` cases. cswap patches the retire to raise; here its manifest
//! rewrite fails for real, on a stash folder the test made read-only.
//!
//! Skipped: `test_a_failed_session_invalidation_does_not_discard_the_adoption`'s
//! `store_write_lands` arm (`_post_backup_write` isn't ported; the other arm is in
//! `grant_faults.rs`).

use super::Harness;
use super::support::{
    EMAIL, chmod, fresh, gate_harness, new, old, posted, queue_invalid_grant, stash_successor,
};
use crate::credentials::{StashReason, backup_username};
use crate::oauth::credential_fingerprint;
use crate::paths::BACKUP_KEYCHAIN_SERVICE;

/// A successor stashed as the persist-lock failure leaves it, `consumedFp` the spent `OLD`.
fn stash_lock_failed(h: &Harness) -> String {
    let consumed_fp = credential_fingerprint(&old());
    stash_successor(
        h,
        &new(),
        StashReason::PersistLockFailed,
        consumed_fp.as_deref(),
    )
}

/// Every manifest rewrite fails while it runs: the stash folder read-only.
fn with_stash_frozen<T>(h: &Harness, run: impl FnOnce() -> T) -> T {
    let dir = h.switcher.credentials_dir.clone();
    chmod(&dir, 0o500);
    let out = run();
    chmod(&dir, 0o700);
    out
}

#[test]
fn row_a_unreadable_entry_does_not_post_the_spent_generation() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let entry_path = h.switcher.store.stash_entry_path(&stash_lock_failed(&h));
    chmod(&entry_path, 0o000);
    queue_invalid_grant(&h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    chmod(&entry_path, 0o600);
    assert!(
        posted(&h).is_empty(),
        "an unreadable entry made adoption fall through"
    );
    assert_eq!(out.error.as_deref(), Some("stash-unreadable"));
    assert!(
        !h.stash().is_empty(),
        "the entry must survive for the next pass"
    );
}

#[test]
fn row_b_control_readable_entry_adopts() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    stash_lock_failed(&h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert!(
        posted(&h).is_empty(),
        "an adopted successor short-circuits the POST"
    );
    assert_eq!(out.error, None);
    assert_eq!(out.credentials.unwrap(), new());
    assert!(h.stash().is_empty(), "stash entry consumed");
}

#[test]
fn row_c_control_nothing_stashed_posts_the_snapshot() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    queue_invalid_grant(&h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(posted(&h), ["rt-old"]);
    assert_eq!(out.error.as_deref(), Some("invalid_grant"));
}

#[test]
fn corrupt_entry_keeps_its_current_behaviour() {
    // Undecodable bytes are gone for good: ABSENT, not unreadable, and never adopted.
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let entry_path = h.switcher.store.stash_entry_path(&stash_lock_failed(&h));
    std::fs::write(&entry_path, "not-valid-base64!!!").unwrap();
    queue_invalid_grant(&h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(posted(&h), ["rt-old"]);
    assert_eq!(out.error.as_deref(), Some("invalid_grant"));
    assert_eq!(
        h.backup("1", EMAIL).unwrap(),
        old(),
        "an empty adopt destroyed the slot"
    );
}

#[test]
fn undecodable_entry_bytes_are_corrupt_not_a_crash() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let entry_path = h.switcher.store.stash_entry_path(&stash_lock_failed(&h));
    std::fs::write(&entry_path, b"\xff\xfegarbled").unwrap();
    queue_invalid_grant(&h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(posted(&h), ["rt-old"]);
    assert_eq!(out.error.as_deref(), Some("invalid_grant"));
    assert_eq!(h.backup("1", EMAIL).unwrap(), old());
}

#[test]
fn row_d_absent_entry_bytes_terminate_instead_of_deferring() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    std::fs::remove_file(h.switcher.store.stash_entry_path(&stash_lock_failed(&h))).unwrap();
    queue_invalid_grant(&h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(
        posted(&h),
        ["rt-old"],
        "bytes already gone can't be adopted by anyone"
    );
    assert_eq!(out.error.as_deref(), Some("invalid_grant"));
}

#[test]
fn absent_entry_is_retired_not_rescanned_forever() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    std::fs::remove_file(h.switcher.store.stash_entry_path(&stash_lock_failed(&h))).unwrap();
    queue_invalid_grant(&h);
    h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert!(
        h.stash().is_empty(),
        "an absent-bytes entry must be retired"
    );
}

#[test]
fn unreadable_row_does_not_starve_a_readable_sibling() {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let unreadable_path = h.switcher.store.stash_entry_path(&stash_lock_failed(&h));
    chmod(&unreadable_path, 0o000);
    let second_id = stash_lock_failed(&h);
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    chmod(&unreadable_path, 0o600);
    assert!(
        posted(&h).is_empty(),
        "a readable sibling must be adopted instead"
    );
    assert_eq!(out.credentials.unwrap(), new());
    assert!(!h.stash().contains_key(&second_id));
}

#[test]
fn a_cas_conflict_stash_does_not_accumulate_forever() {
    // The conflict's row can never match (the store only moves forward): the next scan, on
    // the store's newer generation, retires it.
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let third = fresh("sk-3rd", "rt-3rd");
    let security = h.sandbox.security.clone();
    let written = third.clone();
    h.rebuild_with_http(std::sync::Arc::new(super::ScriptedHttp(
        move |_: &crate::http::Request| {
            security.set(
                BACKUP_KEYCHAIN_SERVICE,
                &backup_username("1", EMAIL),
                &written,
            );
            Ok(super::support::grant("sk-new", "rt-new"))
        },
    )));
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(out.credentials.unwrap(), third);
    assert!(
        !h.stash().is_empty(),
        "the consumed successor must be preserved"
    );
    let adopted = h
        .switcher
        .adopt_stashed_successor("1", EMAIL, &third)
        .unwrap();
    assert_eq!(adopted, None);
    let manifest = h.switcher.store.read_stash_manifest();
    assert!(
        !manifest
            .values()
            .any(|row| row["reason"] == "consume-gate-cas-conflict"),
        "a CAS-conflict entry stays pending forever"
    );
}

fn a_failed_retire_does_not_unwind_a_completed_adoption(retire_fails: bool) {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    stash_lock_failed(&h);
    let adopt = || {
        h.switcher
            .adopt_stashed_successor("1", EMAIL, &old())
            .unwrap()
    };
    let adopted = if retire_fails {
        with_stash_frozen(&h, adopt)
    } else {
        adopt()
    };
    assert_eq!(
        h.backup("1", EMAIL).unwrap(),
        new(),
        "the adoption write lands"
    );
    assert_eq!(
        adopted,
        Some(new()),
        "a completed adoption's credentials not returned: the caller re-POSTs a spent generation"
    );
}

#[test]
fn a_failed_retire_does_not_unwind_a_completed_adoption_retire_ok() {
    a_failed_retire_does_not_unwind_a_completed_adoption(false);
}

#[test]
fn a_failed_retire_does_not_unwind_a_completed_adoption_retire_fails() {
    a_failed_retire_does_not_unwind_a_completed_adoption(true);
}

fn a_failed_housekeeping_retire_does_not_abort_the_scan(dead_row_is_byteless: bool) {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let (reason, consumed_fp) = if dead_row_is_byteless {
        (StashReason::PersistFailed, credential_fingerprint(&old()))
    } else {
        (StashReason::CasConflict, Some("sha256:gone".to_owned()))
    };
    let dead_id = stash_successor(&h, &new(), reason, consumed_fp.as_deref());
    if dead_row_is_byteless {
        std::fs::remove_file(h.switcher.store.stash_entry_path(&dead_id)).unwrap();
    }
    let live_id = stash_lock_failed(&h);
    let order: Vec<String> = h
        .switcher
        .store
        .read_stash_manifest()
        .keys()
        .cloned()
        .collect();
    assert_eq!(order, [dead_id, live_id], "the dead row is scanned first");
    let adopted = with_stash_frozen(&h, || {
        h.switcher
            .adopt_stashed_successor("1", EMAIL, &old())
            .unwrap()
    });
    assert_eq!(adopted, Some(new()), "a failed retire aborted the scan");
    assert_eq!(h.backup("1", EMAIL).unwrap(), new());
}

#[test]
fn a_failed_housekeeping_retire_does_not_abort_the_scan_cas_conflict_row() {
    a_failed_housekeeping_retire_does_not_abort_the_scan(false);
}

#[test]
fn a_failed_housekeeping_retire_does_not_abort_the_scan_byteless_row() {
    a_failed_housekeeping_retire_does_not_abort_the_scan(true);
}

fn a_byteless_non_matching_row_is_retired(bytes_survive: bool) {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &new());
    let entry_id = stash_lock_failed(&h);
    if !bytes_survive {
        std::fs::remove_file(h.switcher.store.stash_entry_path(&entry_id)).unwrap();
    }
    let adopted = h
        .switcher
        .adopt_stashed_successor("1", EMAIL, &new())
        .unwrap();
    assert_eq!(adopted, None, "a non-matching row is never adopted");
    let listed = h
        .switcher
        .store
        .read_stash_manifest()
        .contains_key(&entry_id);
    assert_eq!(
        listed, bytes_survive,
        "a row with bytes is kept; one without is retired"
    );
}

#[test]
fn a_byteless_non_matching_row_is_retired_bytes_survive() {
    a_byteless_non_matching_row_is_retired(true);
}

#[test]
fn a_byteless_non_matching_row_is_retired_bytes_gone() {
    a_byteless_non_matching_row_is_retired(false);
}

fn an_unreadable_non_matching_row_does_not_abort_the_scan(row_a_readable: bool) {
    let h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let row_a = stash_successor(
        &h,
        &new(),
        StashReason::PersistFailed,
        Some("sha256:some-other-generation"),
    );
    let row_b = stash_lock_failed(&h);
    let order: Vec<String> = h
        .switcher
        .store
        .read_stash_manifest()
        .keys()
        .cloned()
        .collect();
    assert_eq!(order, [row_a.clone(), row_b]);
    let path_a = h.switcher.store.stash_entry_path(&row_a);
    if !row_a_readable {
        chmod(&path_a, 0o000);
    }
    let adopted = h
        .switcher
        .adopt_stashed_successor("1", EMAIL, &old())
        .unwrap();
    chmod(&path_a, 0o600);
    assert_eq!(
        adopted,
        Some(new()),
        "an unreadable dead row aborted the scan"
    );
    assert!(
        h.switcher.store.read_stash_manifest().contains_key(&row_a),
        "a row whose bytes are merely unreadable still holds a real credential"
    );
}

#[test]
fn an_unreadable_non_matching_row_does_not_abort_the_scan_readable() {
    an_unreadable_non_matching_row_does_not_abort_the_scan(true);
}

#[test]
fn an_unreadable_non_matching_row_does_not_abort_the_scan_unreadable() {
    an_unreadable_non_matching_row_does_not_abort_the_scan(false);
}

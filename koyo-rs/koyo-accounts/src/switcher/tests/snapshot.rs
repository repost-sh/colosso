//! `accounts_snapshot` as cswap's (0fe22a8) `tests/test_backup_read_pacing.py` drives it,
//! `TestDisplaySitesUseTheCache` (left by the credentials port as the switcher's): a snapshot
//! pass's Keychain reads, counted on the fake `security`. cswap's `switchable` is the slot's
//! `has_backup`.
//!
//! Skipped: `TestStrikeBindingReadsFresh` (`test_a_stale_cached_fingerprint_cannot_heal_a_
//! strike`, `test_a_replaced_backup_heals_with_one_fresh_read`, `test_a_matching_cached_
//! fingerprint_holds_without_a_read`), with the collector's dead-token quarantine
//! (`usage/collect.rs` says why); `tests/test_subscription.py`'s `TestSnapshotField` (the
//! order and defaults of a Python dataclass's fields); and `tests/test_login_expiry.py` and
//! `tests/test_oauth_weekly_only.py`, whose snapshot and collection halves are views and the
//! engine (no test there drives the switcher's collection).

use std::collections::BTreeSet;

use serde_json::json;

use super::Harness;
use super::support::{call_text, fresh};
use crate::credentials::backup_username;
use crate::oauth::USAGE_KEYCHAIN_UNAVAILABLE;
use crate::paths::{BACKUP_KEYCHAIN_SERVICE, CLAUDE_CODE_KEYCHAIN_SERVICE};
use crate::switcher::{AccountsSnapshot, Fetch};
use crate::testing::Answer;

fn email(num: usize) -> String {
    format!("user{num}@example.com")
}

/// cswap's `_cred`.
fn cred(tag: &str) -> String {
    fresh(&format!("sk-{tag}"), &format!("rt-{tag}"))
}

/// cswap's `_switcher`: `n` managed accounts, slot 1 active, Keychain-only backups.
fn accounts(n: usize) -> Harness {
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": email(1), "organizationUuid": "org-1", "accountUuid": "acct-1",
    }}));
    let records: serde_json::Map<String, serde_json::Value> = (1..=n)
        .map(|i| {
            let record = json!({
                "email": email(i), "uuid": format!("acct-{i}"),
                "organizationUuid": format!("org-{i}"), "organizationName": "Org",
                "added": "2024-01-01T00:00:00Z",
            });
            (i.to_string(), record)
        })
        .collect();
    h.write_sequence(&json!({
        "activeAccountNumber": 1,
        "lastUpdated": "2024-01-01T00:00:00Z",
        "sequence": (1..=n).collect::<Vec<_>>(),
        "accounts": records,
    }));
    for i in 1..=n {
        h.set_config_backup(&i.to_string(), &email(i), &json!({}));
        h.set_backup(&i.to_string(), &email(i), &cred(&format!("backup{i}")));
    }
    h.set_live(&cred("live1"));
    h.sandbox.security.clear_calls();
    h
}

/// The Keychain calls naming slot `num`'s backup.
fn backup_calls(h: &Harness, num: usize) -> usize {
    let account = format!("\"{}\"", backup_username(&num.to_string(), &email(num)));
    let bare = backup_username(&num.to_string(), &email(num));
    h.sandbox
        .security
        .calls()
        .iter()
        .map(call_text)
        .filter(|text| {
            text.contains(BACKUP_KEYCHAIN_SERVICE)
                && (text.contains(&account) || text.split(' ').any(|word| word == bare))
        })
        .count()
}

fn live_calls(h: &Harness) -> usize {
    h.sandbox
        .security
        .calls()
        .iter()
        .filter(|call| call_text(call).contains(CLAUDE_CODE_KEYCHAIN_SERVICE))
        .count()
}

fn snapshot(h: &Harness) -> AccountsSnapshot {
    let nothing = BTreeSet::new();
    h.switcher
        .accounts_snapshot(Fetch::Only {
            only: &nothing,
            scheduled: false,
        })
        .unwrap()
}

#[test]
fn a_snapshot_pass_spawns_once_per_slot_then_only_the_live_read() {
    let n = 5;
    let h = accounts(n);
    let snap = snapshot(&h);
    assert_eq!(
        snap.accounts
            .iter()
            .map(|a| a.slot.has_backup)
            .collect::<Vec<_>>(),
        vec![true; n]
    );
    assert!(snap.accounts.iter().all(|a| a.usage.sentinel.is_none()));
    assert_eq!(snap.current.as_deref(), Some("1"));
    assert_eq!(live_calls(&h), 1);
    assert_eq!((1..=n).map(|i| backup_calls(&h, i)).sum::<usize>(), n);
    assert_eq!(backup_calls(&h, 1), 1);
    h.sandbox.security.clear_calls();
    let snap = snapshot(&h);
    assert!(snap.accounts.iter().all(|a| a.slot.has_backup));
    assert_eq!(
        h.sandbox.security.calls().len(),
        1,
        "a repaint within the TTL must spawn for the live item only"
    );
    assert_eq!(live_calls(&h), 1);
}

#[test]
fn a_locked_keychain_is_probed_once_per_cooldown() {
    let n = 5;
    let h = accounts(n);
    h.sandbox.security.answer_all(Answer::Exit(36));
    let snap = snapshot(&h);
    let calls = h.sandbox.security.calls().len();
    assert!(calls <= 3, "{calls} calls");
    assert!((2..=n).map(|i| backup_calls(&h, i)).sum::<usize>() <= 1);
    let idle: Vec<_> = snap.accounts.iter().filter(|a| !a.is_active).collect();
    assert!(
        idle.iter()
            .all(|a| a.usage.sentinel.as_deref() == Some(USAGE_KEYCHAIN_UNAVAILABLE))
    );
    assert!(idle.iter().all(|a| !a.slot.has_backup));
    h.sandbox.security.clear_calls();
    snapshot(&h);
    assert!(
        h.sandbox.security.calls().is_empty(),
        "inside the cooldown a pass must not spawn"
    );
}

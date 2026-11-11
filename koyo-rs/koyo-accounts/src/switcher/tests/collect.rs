//! cswap's `tests/test_switcher.py` (0fe22a8) for one collection pass as the views drive it:
//! `TestListAccountsUsage`'s collection cases (cswap drives them through `list_accounts`;
//! here through `usage_entries_by_account`, the pass `list_accounts` runs, with its rows'
//! entries for its printed lines), `TestBackupUnreadableDisplay`, and
//! `TestMacosKeychainFallback.test_list_active_shows_keychain_unavailable`. Skipped with
//! `usage_fetch_stamps`, which isn't ported: `TestUsageFetchStamps`
//! (`test_stamps_reflect_store_without_fetching`).
//!
//! Adapted: `test_list_never_writes_live_while_claude_code_running` (cswap's idle fetch
//! refreshes through the gate; Koyo's pass writes neither the live store nor a backup) and
//! `test_expired_active_is_no_longer_statically_gated` (no static gate, and the fetch path's
//! own answer is `token expired`, not a refresh). Skipped as printing (`list_accounts`,
//! `status`, `_token_status_lines` aren't ported): `test_list_shows_usage`,
//! `test_list_shows_alias_before_email`, `test_list_shows_usage_null_reset`,
//! `test_list_shows_token_status_when_requested`, the seven `test_token_status_lines_*`, and
//! `TestStatusCache` (its collection rules are the cases below). The `test_replan_new_active_*`
//! pair is the switch's, in `switch_to.rs`.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::Harness;
use super::access_creds;
use super::support::{EMAIL, bearers, chmod, info, posted, queue_usage, usage_requests};
use crate::Clock as _;
use crate::oauth::{USAGE_KEYCHAIN_UNAVAILABLE, USAGE_NO_CREDENTIALS, USAGE_TOKEN_EXPIRED};
use crate::paths::CLAUDE_CODE_KEYCHAIN_SERVICE;
use crate::switcher::Fetch;
use crate::switcher::info::AccountInfo;
use crate::testing::{Answer, entry_as};
use crate::usage_store::{Identities, UsageEntry};

/// cswap's list setup: `sample_sequence_data` with slot 1 the live `test@example.com`, the
/// live credential `sk-active` and slot 2's backup `sk-backup` (access-only, no expiry).
fn list_harness() -> Harness {
    let h = super::support::gate_harness();
    h.mock_claude_config();
    h.set_live(&access_creds("sk-active"));
    h.set_backup("2", "account2@example.com", &access_creds("sk-backup"));
    h
}

fn identities() -> Identities {
    Identities::from([
        ("1".into(), (EMAIL.into(), String::new())),
        ("2".into(), ("account2@example.com".into(), String::new())),
    ])
}

/// A reading the store holds for `num`, as cswap's `UsageStore.record` of a `FetchRecord`,
/// `age_s` old.
fn recorded(h: &Harness, num: &str, usage: Value, age_s: f64) {
    super::support::recorded(h, num, usage, &identities(), age_s);
}

/// The endpoint's answer for `pct` on both windows... as the list tests' `usage_result`.
fn answer(five: f64, seven: f64) -> Value {
    json!({"five_hour": {"utilization": five}, "seven_day": {"utilization": seven}})
}

fn entries(h: &Harness, fetch: Fetch<'_>) -> std::collections::BTreeMap<String, UsageEntry> {
    h.switcher.usage_entries_by_account(fetch).unwrap()
}

/// A pass restricted to `only`, as the engine's scheduler asks for one.
fn only(only: &BTreeSet<String>) -> Fetch<'_> {
    Fetch::Only {
        only,
        scheduled: false,
    }
}

fn pct(entry: &UsageEntry, window: &str) -> Value {
    entry.last_good.as_ref().unwrap()[window]["pct"].clone()
}

#[test]
fn list_no_credentials() {
    let h = super::support::gate_harness();
    h.mock_claude_config();
    let entries = entries(&h, Fetch::Any);
    for num in ["1", "2"] {
        assert_eq!(
            entries[num].sentinel.as_deref(),
            Some(USAGE_NO_CREDENTIALS),
            "{num}"
        );
    }
}

#[test]
fn list_never_writes_live_while_claude_code_running() {
    let h = list_harness();
    h.set_backup(
        "2",
        "account2@example.com",
        &super::oauth_creds("sk-backup", "rt-orig"),
    );
    queue_usage(&h, &json!({}));
    queue_usage(&h, &json!({}));
    h.sandbox.security.clear_calls();
    entries(&h, Fetch::Any);
    let wrote = h
        .sandbox
        .security
        .calls()
        .iter()
        .any(|call| super::support::call_text(call).starts_with("add-generic-password"));
    assert!(!wrote, "a collection pass wrote a credential");
    assert!(posted(&h).is_empty(), "a collection pass refreshed a token");
    assert_eq!(h.live().unwrap(), access_creds("sk-active"));
}

#[test]
fn list_uses_cached_usage() {
    let h = list_harness();
    recorded(
        &h,
        "1",
        json!({"five_hour": {"pct": 25}, "seven_day": {"pct": 60}}),
        0.,
    );
    recorded(
        &h,
        "2",
        json!({"five_hour": {"pct": 80}, "seven_day": {"pct": 90}}),
        0.,
    );
    let entries = entries(&h, Fetch::Any);
    assert_eq!(usage_requests(&h), 0);
    assert_eq!(pct(&entries["1"], "five_hour"), 25);
    assert_eq!(pct(&entries["2"], "five_hour"), 80);
}

#[test]
fn list_refetches_stale_entries() {
    let h = list_harness();
    recorded(&h, "1", json!({"five_hour": {"pct": 25}}), 400.);
    queue_usage(&h, &answer(10., 50.));
    queue_usage(&h, &answer(10., 50.));
    let entries = entries(&h, Fetch::Any);
    assert_eq!(usage_requests(&h), 2);
    assert_eq!(pct(&entries["1"], "five_hour"), 10.);
}

#[test]
fn on_demand_pass_persists_poll_plans() {
    let h = list_harness();
    queue_usage(&h, &answer(10., 50.));
    queue_usage(&h, &answer(10., 50.));
    entries(&h, Fetch::Any);
    let stored = h.switcher.usage_store.entries(&identities(), &[]);
    for num in ["1", "2"] {
        assert!(stored[num].next_poll_at.is_some(), "{num}");
        assert!(stored[num].poll_interval_s.is_some(), "{num}");
    }
}

#[test]
fn on_demand_pass_respects_poll_plans() {
    let h = list_harness();
    recorded(&h, "1", json!({"five_hour": {"pct": 25}}), 400.);
    let now = h.sandbox.clock.now();
    h.switcher
        .usage_store
        .set_poll_plan(
            &[("1".to_owned(), (Some(now + 600.), Some(600.)))].into(),
            &identities(),
        )
        .unwrap();
    queue_usage(&h, &answer(10., 50.));
    let entries = entries(&h, Fetch::Any);
    assert_eq!(usage_requests(&h), 1, "slot 1's plan isn't due");
    assert_eq!(pct(&entries["1"], "five_hour"), 25);
}

#[test]
fn on_demand_pass_repairs_reset_parked_exhausted_plan() {
    let h = list_harness();
    let exhausted = json!({
        "five_hour": {"pct": 25},
        "seven_day": {"pct": 100, "resets_at": "2099-01-01T00:00:00Z"},
    });
    recorded(&h, "1", exhausted, 400.);
    let now = h.sandbox.clock.now();
    h.switcher
        .usage_store
        .set_poll_plan(
            &[("1".to_owned(), (Some(now + 86_400.), Some(300.)))].into(),
            &identities(),
        )
        .unwrap();
    queue_usage(&h, &answer(5., 10.));
    queue_usage(&h, &answer(5., 10.));
    let entries = entries(&h, Fetch::Any);
    assert_eq!(usage_requests(&h), 2, "the reset-parked plan was repaired");
    assert_eq!(pct(&entries["1"], "seven_day"), 10.);
    let stored = entry_as(&h.switcher.usage_store, "1", &identities());
    assert!(stored.next_poll_at.unwrap() < now + 86_400.);
}

#[test]
fn list_fetch_set_restricts_fetches() {
    let h = list_harness();
    queue_usage(&h, &answer(10., 50.));
    entries(&h, only(&BTreeSet::new()));
    assert_eq!(usage_requests(&h), 0);
    entries(&h, only(&BTreeSet::from(["2".to_owned()])));
    assert_eq!(
        bearers(&h, crate::oauth::USAGE_URL),
        ["sk-backup"],
        "only slot 2 was fetched"
    );
}

#[test]
fn idle_slot_unreadable_backup_shows_keychain_unavailable() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    h.sandbox.security.answer_all(Answer::Exit(36));
    h.switcher
        .store
        .read_account_credentials_ex("2", "b@example.com", false);
    let row = info("2", "b@example.com", "", false, "");
    assert_eq!(
        h.switcher.static_usage_sentinel(&row),
        Some(USAGE_KEYCHAIN_UNAVAILABLE)
    );
}

#[test]
fn idle_slot_absent_backup_still_no_credentials() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    let row = info("2", "b@example.com", "", false, "");
    assert_eq!(
        h.switcher.static_usage_sentinel(&row),
        Some(USAGE_NO_CREDENTIALS)
    );
}

#[test]
fn active_slot_unreadable_credential_shows_keychain_unavailable_on_linux() {
    // cswap's Linux case: `.credentials.json` there but unreadable, no Keychain failure. On
    // the Mac that is the file fallback of an absent Keychain item.
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    h.set_live(&access_creds("sk-live"));
    let active = h.switcher.store.read_active_credentials();
    let ok = AccountInfo {
        active: Some(active.verdict()),
        ..info(
            "1",
            "a@example.com",
            "",
            true,
            active.value.as_deref().unwrap_or_default(),
        )
    };
    assert_eq!(
        h.switcher.static_usage_sentinel(&ok),
        None,
        "control broken"
    );
    h.sandbox
        .security
        .remove(CLAUDE_CODE_KEYCHAIN_SERVICE, &h.user());
    std::fs::write(h.credentials_file(), access_creds("sk-live")).unwrap();
    chmod(&h.credentials_file(), 0o000);
    let bad = h.switcher.store.read_active_credentials();
    chmod(&h.credentials_file(), 0o600);
    assert_eq!(
        bad.value, None,
        "premise: an unreadable file gives value None"
    );
    assert!(
        !bad.keychain_unavailable,
        "premise: the Keychain didn't fail"
    );
    let row = AccountInfo {
        active: Some(bad.verdict()),
        ..info("1", "a@example.com", "", true, "")
    };
    assert_eq!(
        h.switcher.static_usage_sentinel(&row),
        Some(USAGE_KEYCHAIN_UNAVAILABLE)
    );
}

#[test]
fn list_active_shows_keychain_unavailable() {
    // A locked Keychain read "no credentials" and nudged the person into a needless re-login.
    let h = super::support::gate_harness();
    h.mock_claude_config();
    h.sandbox.security.answer_all(Answer::Exit(36));
    assert!(!h.credentials_file().exists());
    let entries = entries(&h, Fetch::Any);
    assert_eq!(
        entries["1"].sentinel.as_deref(),
        Some(USAGE_KEYCHAIN_UNAVAILABLE)
    );
}

#[test]
fn expired_active_is_no_longer_statically_gated() {
    let h = super::support::gate_harness();
    h.mock_claude_config();
    let expired = super::support::creds("sk-active", "rt-orig", json!(1000));
    let row = info("1", EMAIL, "", true, &expired);
    assert_eq!(
        h.switcher.static_usage_sentinel(&row),
        None,
        "statically gated"
    );
    let entry = h
        .switcher
        .collect_usage_entries(&[row], Fetch::Any)
        .unwrap()
        .remove("1")
        .unwrap();
    assert_eq!(entry.sentinel.as_deref(), Some(USAGE_TOKEN_EXPIRED));
    assert_eq!(usage_requests(&h), 0);
    assert!(posted(&h).is_empty());
}

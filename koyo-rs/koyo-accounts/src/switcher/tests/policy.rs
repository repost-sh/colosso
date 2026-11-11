//! cswap's `tests/test_switcher.py` (0fe22a8) `TestDisableEnableAccount`, and the disable-flag
//! cases of `tests/test_account_policy_store.py`'s `TestPolicyFromData`, on
//! `_disabled_from_data` (Koyo's policy is the flag alone). `TestAccountPolicies` is left out
//! with `account_policies`. What cswap prints is asserted on the returned
//! [`HoldOutcome`](crate::switcher::HoldOutcome).

use super::{Harness, make_live, seed};
use crate::AccountsError;
use crate::switcher::Switcher;

fn fleet(count: u64) -> Harness {
    let h = Harness::new();
    for n in 1..=count {
        seed(&h, n, &format!("account-{n}@example.test"));
    }
    h
}

#[test]
fn disable_sets_flag_and_excludes_from_rotation() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    seed(&h, 3, "c@example.com");
    let outcome = h.switcher.set_account_disabled("2", true).unwrap();
    assert!(outcome.changed);
    assert!(h.switcher.is_account_disabled("2").unwrap());
    assert_eq!(h.switcher.disabled_account_numbers().unwrap(), ["2"]);
    assert_eq!(h.switcher.switchable_account_numbers().unwrap(), ["1", "3"]);
    assert_eq!(h.accounts()["2"]["disabled"], true);
}

#[test]
fn enable_clears_flag_and_restores_position() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    seed(&h, 3, "c@example.com");
    h.switcher.set_account_disabled("2", true).unwrap();
    let outcome = h.switcher.set_account_disabled("2", false).unwrap();
    assert!(outcome.changed);
    assert!(!h.switcher.is_account_disabled("2").unwrap());
    assert!(h.switcher.disabled_account_numbers().unwrap().is_empty());
    // In its original place, not appended.
    assert_eq!(
        h.switcher.switchable_account_numbers().unwrap(),
        ["1", "2", "3"]
    );
    assert!(h.accounts()["2"].get("disabled").is_none());
}

#[test]
fn disable_by_email() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    h.switcher
        .set_account_disabled("b@example.com", true)
        .unwrap();
    assert!(h.switcher.is_account_disabled("2").unwrap());
}

#[test]
fn repeated_disable_is_noop() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    h.switcher.set_account_disabled("2", true).unwrap();
    // cswap prints "already disabled".
    assert!(!h.switcher.set_account_disabled("2", true).unwrap().changed);
    assert!(h.switcher.is_account_disabled("2").unwrap());
}

#[test]
fn enable_when_not_disabled_is_noop() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    assert!(!h.switcher.set_account_disabled("1", false).unwrap().changed);
}

#[test]
fn disable_unknown_account_raises() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    assert!(matches!(
        h.switcher.set_account_disabled("99", true),
        Err(AccountsError::AccountNotFound(_))
    ));
}

#[test]
fn disable_with_no_managed_accounts_raises() {
    let h = Harness::new();
    assert!(matches!(
        h.switcher.set_account_disabled("1", true),
        Err(AccountsError::Config(_))
    ));
}

#[test]
fn explicit_switch_to_disabled_still_works() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    h.switcher.set_account_disabled("2", true).unwrap();
    make_live(&h, "a@example.com", 1);
    h.switcher.switch_to("2").unwrap();
    assert_eq!(h.sequence()["activeAccountNumber"], 2);
}

#[test]
fn remove_then_readd_clears_disabled() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    h.switcher.set_account_disabled("2", true).unwrap();
    let mut data = h.sequence();
    data["accounts"].as_object_mut().unwrap().shift_remove("2");
    data["sequence"] = serde_json::json!([1]);
    h.write_sequence(&data);
    seed(&h, 2, "b@example.com");
    assert!(!h.switcher.is_account_disabled("2").unwrap());
}

#[test]
fn bare_add_keeps_account_parked() {
    // cswap's `test_slot_refresh_keeps_account_parked` on the path Koyo has: re-adding the
    // live account refreshes its slot in place, so the park survives.
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    h.switcher.set_account_disabled("2", true).unwrap();
    make_live(&h, "b@example.com", 2);
    let added = h.switcher.add_account().unwrap();
    assert!(added.updated);
    assert_eq!(added.account_num, "2");
    assert!(h.switcher.is_account_disabled("2").unwrap());
    assert_eq!(h.switcher.switchable_account_numbers().unwrap(), ["1"]);
}

#[test]
fn disable_active_account_warns_but_sets_flag() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com"); // becomes active
    seed(&h, 2, "b@example.com");
    let outcome = h.switcher.set_account_disabled("1", true).unwrap();
    assert!(outcome.was_active);
    assert!(h.switcher.is_account_disabled("1").unwrap());
}

#[test]
fn disable_last_rotation_account_warns() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    assert!(
        !h.switcher
            .set_account_disabled("1", true)
            .unwrap()
            .rotation_empty
    );
    // Now nothing is left in rotation.
    assert!(
        h.switcher
            .set_account_disabled("2", true)
            .unwrap()
            .rotation_empty
    );
}

#[test]
fn missing_record_yields_the_default() {
    let h = fleet(3);
    let data = h.switcher.get_sequence_data().unwrap().unwrap();
    assert!(!Switcher::disabled_from_data(&data, "99"));
}

#[test]
fn record_without_policy_keys_yields_the_default() {
    let h = fleet(3);
    let data = h.switcher.get_sequence_data().unwrap().unwrap();
    assert!(!Switcher::disabled_from_data(&data, "1"));
}

#[test]
fn setting_then_unsetting_restores_the_record_exactly() {
    // A disable and an enable pop the key rather than leave `false`.
    let h = fleet(2);
    let before = h.accounts()["1"].clone();
    h.switcher.set_account_disabled("1", true).unwrap();
    h.switcher.set_account_disabled("1", false).unwrap();
    assert_eq!(h.accounts()["1"], before);
}

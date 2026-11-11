//! cswap's `tests/test_switcher.py` (0fe22a8) through `switch_to` and `remove_account`:
//! `TestSwitchToSelfSlotAndForce.test_switch_to_current_slot_is_noop_preserving_backup`,
//! `TestSelfSwitchProvenance`, `TestSwitchSkipsBrokenSlots`'s `switch_to` cases,
//! `TestClaudeCodeLockCooperation`, `TestSwitchUnreadableBackup`,
//! `TestListAccountsUsage.test_replan_new_active_*`, and the `remove_account` cases of
//! `TestRemoveAccountPrunesMappings` and `TestSwitchRemoveGatesAcceptAlias`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::switch::two_accounts;
use super::{Harness, make_live, oauth_creds, profile, sample_sequence_data, seed, seed_with};
use crate::claude_locks::{config_lock_dir, credentials_lock_dir, oauth_refresh_lock_dir};
use crate::keychain::{KeychainError, Output, Security};
use crate::switcher::AccountRef;
use crate::testing::{Answer, FakeSecurity};
use crate::usage_store::{FetchRecord, Identities, one_identity};
use crate::{AccountsError, Clock as _, poll_policy};

#[test]
fn switch_to_current_slot_is_noop_preserving_backup() {
    // A self-switch neither poisons a freshly imported backup nor rewrites the live login.
    let h = two_accounts(&mut sample_sequence_data());
    let imported = oauth_creds("sk-imported-1", "rt-imported-1");
    let live = oauth_creds("sk-live-1", "rt-live-1");
    h.set_backup("1", "test@example.com", &imported);
    h.set_backup("2", "account2@example.com", &oauth_creds("sk-2", "rt-2"));
    h.set_live(&live);
    let result = h.switcher.switch_to("1").unwrap();
    assert!(!result.switched);
    assert_eq!(result.reason.as_str(), "already-active");
    assert!(result.message.contains("Already on") && result.message.contains("Account-1"));
    assert_eq!(h.backup("1", "test@example.com").unwrap(), imported);
    assert_eq!(h.live().unwrap(), live);
}

#[test]
fn matching_self_switch_is_noop() {
    let h = two_accounts(&mut sample_sequence_data());
    let backup = oauth_creds("sk-1", "rt-1");
    h.set_backup("1", "test@example.com", &backup);
    h.set_live(&backup);
    let result = h.switcher.switch_to("1").unwrap();
    assert!(!result.switched);
    assert_eq!(result.reason.as_str(), "already-active");
    let here = Some(AccountRef::new(Some(1), "test@example.com"));
    assert_eq!((result.from, result.to), (here.clone(), here));
}

#[test]
fn diverged_unresolvable_self_switch_noops_silently() {
    // Endpoint trouble is invisible on the self-switch path, and nothing moves.
    let h = two_accounts(&mut sample_sequence_data());
    let backup = oauth_creds("sk-1", "rt-1");
    let diverged = oauth_creds("sk-1-new", "rt-1-rotated");
    h.set_backup("1", "test@example.com", &backup);
    h.set_live(&diverged);
    let result = h.switcher.switch_to("1").unwrap();
    assert!(!result.switched);
    assert_eq!(result.reason.as_str(), "already-active");
    assert!(result.warnings.is_empty());
    assert_eq!(h.live().unwrap(), diverged);
    assert_eq!(h.backup("1", "test@example.com").unwrap(), backup);
}

#[test]
fn diverged_resolved_self_switch_reconciles() {
    // A rotation proven to be the slot's own is re-synced into its backup.
    let h = two_accounts(&mut sample_sequence_data());
    let rotated = oauth_creds("sk-1-new", "rt-1-rotated");
    h.set_backup("1", "test@example.com", &oauth_creds("sk-1", "rt-1"));
    h.set_config_backup(
        "1",
        "test@example.com",
        &json!({"oauthAccount": {"emailAddress": "test@example.com", "accountUuid": "uuid-1"}}),
    );
    h.set_live(&rotated);
    h.queue_profile(&profile("uuid-1", Some("test@example.com"), Some("")));
    let result = h.switcher.switch_to("1").unwrap();
    assert_eq!(h.backup("1", "test@example.com").unwrap(), rotated);
    assert!(!result.switched);
    assert_eq!(result.to.unwrap().number, Some(1));
}

fn broken_slot_two(creds: bool, config: bool) -> Harness {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed_with(&h, 2, "b@example.com", creds, config);
    make_live(&h, "a@example.com", 1);
    h
}

#[test]
fn switch_to_missing_credentials_actionable_error() {
    let h = broken_slot_two(false, true);
    let Err(AccountsError::Switch(why)) = h.switcher.switch_to("2") else {
        panic!("a slot with no credential can't be switched to");
    };
    assert!(why.contains("has no stored credentials"), "{why}");
}

#[test]
fn switch_to_missing_config_actionable_error() {
    let h = broken_slot_two(true, false);
    let Err(AccountsError::Switch(why)) = h.switcher.switch_to("2") else {
        panic!("a slot with no config backup can't be switched to");
    };
    assert!(why.contains("has no stored config backup"), "{why}");
}

/// `security`, recording whether Claude Code's locks are held at each write of the live item.
struct LockSpy {
    inner: Arc<FakeSecurity>,
    locks: Vec<PathBuf>,
    seen: Mutex<Vec<Vec<bool>>>,
}

impl Security for LockSpy {
    fn run(&self, args: &[&str], input: Option<&str>) -> Result<Output, KeychainError> {
        if input.is_some_and(|line| {
            line.starts_with("add-generic-password") && line.contains("\"Claude Code-credentials\"")
        }) {
            let held = self.locks.iter().map(|lock| lock.is_dir()).collect();
            self.seen.lock().unwrap().push(held);
        }
        self.inner.run(args, input)
    }
}

#[test]
fn switch_holds_both_cc_locks_at_write_time() {
    let mut h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    make_live(&h, "a@example.com", 1);
    let env = h.sandbox.env();
    let locks = vec![
        oauth_refresh_lock_dir(&env),
        credentials_lock_dir(&env),
        config_lock_dir(&env),
    ];
    let spy = Arc::new(LockSpy {
        inner: h.sandbox.security.clone(),
        locks: locks.clone(),
        seen: Mutex::new(Vec::new()),
    });
    h.rebuild_with_security(spy.clone());
    h.switcher.switch_to("2").unwrap();
    assert_eq!(h.sequence()["activeAccountNumber"], 2);
    let seen = spy.seen.lock().unwrap();
    assert!(!seen.is_empty() && seen.iter().all(|held| held.iter().all(|&held| held)));
    // Released after the switch.
    assert!(locks.iter().all(|lock| !lock.exists()));
}

#[test]
fn preheld_cc_lock_fails_cleanly_without_mutation() {
    let mut h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    make_live(&h, "a@example.com", 1);
    h.switcher.claude_lock_timeout = Some(Duration::from_millis(300));
    let held = credentials_lock_dir(&h.sandbox.env());
    // A fresh mtime: a live Claude Code refresh.
    std::fs::create_dir(&held).unwrap();
    let live_before = h.live();
    assert!(matches!(
        h.switcher.switch_to("2"),
        Err(AccountsError::ClaudeCodeLockTimeout(_))
    ));
    // Nothing moved: the locks are taken before any write.
    assert_eq!(h.sequence()["activeAccountNumber"], 1);
    assert_eq!(h.live(), live_before);
    // The holder's lock is left alone.
    assert!(held.is_dir());
}

#[test]
fn switch_to_unreadable_backup_says_keychain() {
    // The direct-activation branch: the live identity (test@) matches no slot.
    let h = Harness::new();
    h.mock_claude_config();
    h.write_sequence(&sample_sequence_data());
    h.sandbox.security.answer_all(Answer::Exit(51));
    let Err(AccountsError::Switch(why)) = h.switcher.switch_to("2") else {
        panic!("an unreadable backup must fail the switch");
    };
    let why = why.to_lowercase();
    assert!(why.contains("keychain"), "{why}");
    // The remedy must not be a re-add.
    assert!(!why.contains("add the current login"), "{why}");
}

#[test]
fn normal_path_switch_to_unreadable_backup_says_keychain() {
    // The ordinary switch, a live login that resolves to slot 1. cswap's store writes the
    // outgoing backup to a file while the Keychain is unreadable and fails on the target's
    // read; Koyo has no file, so the switch stops at the outgoing backup, with nothing changed.
    // Either way it names the Keychain and never sends the person to re-add.
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": "account1@example.com", "accountUuid": "uuid-1",
        "organizationUuid": "", "organizationName": "",
    }}));
    let live = json!({"claudeAiOauth": {
        "accessToken": "sk-live", "refreshToken": "rt-live", "expiresAt": 9_999_999_999_000_u64,
    }})
    .to_string();
    std::fs::write(h.credentials_file(), &live).unwrap();
    h.set_backup("2", "account2@example.com", &live);
    h.set_config_backup(
        "2",
        "account2@example.com",
        &json!({"oauthAccount": {
            "emailAddress": "account2@example.com", "accountUuid": "uuid-2",
            "organizationUuid": "", "organizationName": "",
        }}),
    );
    assert_eq!(
        h.switcher.current_account_number().unwrap().as_deref(),
        Some("1")
    );
    h.sandbox.security.answer_all(Answer::Exit(51));
    let error = h.switcher.switch_to("2").unwrap_err();
    assert!(
        matches!(
            error,
            AccountsError::Switch(_) | AccountsError::CredentialWrite(_)
        ),
        "{error:?}"
    );
    let why = error.to_string().to_lowercase();
    assert!(why.contains("keychain"), "{why}");
    assert!(!why.contains("add the current login"), "{why}");
    assert_eq!(h.sequence()["activeAccountNumber"], 1);
    assert_eq!(std::fs::read_to_string(h.credentials_file()).unwrap(), live);
}

fn usage() -> serde_json::Value {
    json!({"five_hour": {"pct": 10}})
}

#[test]
fn replan_new_active_pulls_candidate_plan_to_floor() {
    // A plan learned while the account idled as a candidate must not gate it once active.
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    let store = &h.switcher.usage_store;
    let now = h.sandbox.clock.now();
    let ident1 = one_identity("1", "a@x.com", "");
    let record = |num: &str, identities: &Identities| {
        store
            .record(
                &BTreeMap::from([(num.to_owned(), FetchRecord::success(Some(usage())))]),
                identities,
                None,
                None,
            )
            .unwrap();
    };
    let plan = |num: &str, at: f64, interval: f64, identities: &Identities| {
        store
            .set_poll_plan(
                &BTreeMap::from([(num.to_owned(), (Some(at), Some(interval)))]),
                identities,
            )
            .unwrap();
    };
    record("1", &ident1);
    plan("1", now + 600., 600., &ident1);
    h.switcher.replan_new_active("1", "a@x.com", "");
    let entry = store.entries(&ident1, &[]).remove("1").unwrap();
    assert_eq!(entry.poll_interval_s, Some(poll_policy::MIN_INTERVAL_S));
    assert!(entry.next_poll_at.unwrap() <= now + poll_policy::MIN_INTERVAL_S + 1.);

    // An already-eager plan is never pushed later.
    plan("1", now + 60., 60., &ident1);
    h.switcher.replan_new_active("1", "a@x.com", "");
    assert_eq!(
        store
            .entries(&ident1, &[])
            .remove("1")
            .unwrap()
            .poll_interval_s,
        Some(60.)
    );

    // A never-measured account gets no plan.
    let ident2 = one_identity("2", "b@x.com", "");
    h.switcher.replan_new_active("2", "b@x.com", "");
    assert_eq!(
        store
            .entries(&ident2, &[])
            .remove("2")
            .unwrap()
            .next_poll_at,
        None
    );

    // An already-old measurement comes due now, not 180 s from now.
    h.sandbox.clock.set(now - 400.);
    record("2", &ident2);
    h.sandbox.clock.set(now);
    plan("2", now + 600., 600., &ident2);
    h.switcher.replan_new_active("2", "b@x.com", "");
    assert!(
        store
            .entries(&ident2, &[])
            .remove("2")
            .unwrap()
            .next_poll_at
            .unwrap()
            <= now + 1.
    );
}

#[test]
fn replan_new_active_failure_is_logged_not_raised() {
    // The switch it rides on has committed: a cache failure never surfaces.
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    let ident1 = one_identity("1", "a@x.com", "");
    let store = &h.switcher.usage_store;
    store
        .record(
            &BTreeMap::from([("1".to_owned(), FetchRecord::success(Some(usage())))]),
            &ident1,
            None,
            None,
        )
        .unwrap();
    // The store's lock can't be opened: its write fails.
    let lock = crate::paths::store::cache_dir(&h.sandbox.env()).join(".usage.lock");
    std::fs::set_permissions(&lock, std::os::unix::fs::PermissionsExt::from_mode(0o400)).unwrap();
    h.switcher.replan_new_active("1", "a@x.com", "");
    std::fs::set_permissions(&lock, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    assert_eq!(
        store
            .entries(&ident1, &[])
            .remove("1")
            .unwrap()
            .next_poll_at,
        None
    );
}

#[test]
fn a_switch_reports_where_it_came_from_and_went() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed(&h, 2, "b@example.com");
    make_live(&h, "a@example.com", 1);
    let result = h.switcher.switch_to("2").unwrap();
    assert!(result.switched);
    assert_eq!(result.reason.as_str(), "switched");
    assert_eq!(result.from, Some(AccountRef::new(Some(1), "a@example.com")));
    assert_eq!(result.to, Some(AccountRef::new(Some(2), "b@example.com")));
    assert_eq!(result.message, "Switched to Account-2 (b@example.com)");
    assert_eq!(h.live_access_token(), "sk-2");
}

#[test]
fn remove_account_removes_the_slot() {
    // cswap's `test_remove_account_prunes_mappings`, the mappings left out.
    let h = Harness::new();
    seed(&h, 1, "a@x.com");
    let removed = h.switcher.remove_account("1").unwrap();
    assert_eq!(removed.email, "a@x.com");
    assert!(removed.was_active);
    assert!(h.accounts().is_empty());
    assert_eq!(h.sequence()["sequence"], json!([]));
    assert!(h.backup("1", "a@x.com").is_none());
    assert!(h.config_backup("1", "a@x.com").is_empty());
}

#[test]
fn remove_account_invalid_identifier_still_raises_validation() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    assert!(matches!(
        h.switcher.remove_account("not an email or alias!"),
        Err(AccountsError::Validation(_))
    ));
}

#[test]
fn switch_to_invalid_identifier_raises_validation() {
    // cswap's `test_switch_to_unknown_alias_raises_account_not_found_not_validation`, its
    // format gate.
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    assert!(matches!(
        h.switcher.switch_to("not an email or alias!"),
        Err(AccountsError::Validation(_))
    ));
    assert!(matches!(
        h.switcher.switch_to("nobody@example.com"),
        Err(AccountsError::AccountNotFound(_))
    ));
}

//! cswap's `tests/test_switcher.py` (0fe22a8) for the refresh gate under faults and races:
//! `TestGateUltraReviewFixes`'s persist failures and concurrent gates, the store-rejects arm of
//! `test_a_failed_session_invalidation_does_not_discard_the_adoption`, and
//! `TestUnreadableBackupIsNotAbsent.test_backup_unreadable_after_post_is_not_a_removed_slot`.
//! cswap's patched writes that raise are Keychain writes the [`Faulty`] `security` refuses;
//! a stash that fails is a stash folder made read-only.
//!
//! `test_concurrent_gates_consume_only_one_grant` waits 0.3 s for the second gate to reach the
//! consume lock; here the second gate runs while the first is held inside its POST, with a
//! short wait for the lock, so it finds it busy every time, and runs again once the first is
//! done.
//!
//! [`Faulty`]: super::support::Faulty

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::support::{
    EMAIL, backup_write_refuser, chmod, gate_harness, grant, new, old, sent_refresh_token, tokens,
};
use super::{Harness, ScriptedHttp};
use crate::http::{HttpError, Request};
use crate::oauth::OAUTH_TOKEN_URL;
use crate::paths::BACKUP_KEYCHAIN_SERVICE;
use crate::switcher::Switcher;
use crate::testing::Answer;

/// The stash rows' reasons.
fn reasons(h: &Harness) -> Vec<String> {
    h.stash()
        .values()
        .filter_map(|row| row["reason"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn persist_oserror_after_post_stashes_and_never_raises() {
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let (security, armed) = backup_write_refuser(&h, "1", EMAIL);
    let http = ScriptedHttp(move |_: &Request| {
        armed.store(true, Ordering::SeqCst);
        Ok(grant("sk-new", "rt-new"))
    });
    h.rebuild_with(security, Arc::new(http));
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(out.error.as_deref(), Some("transient"));
    assert_eq!(
        tokens(&out.credentials.unwrap()),
        ("sk-new".into(), "rt-new".into())
    );
    assert_eq!(
        reasons(&h),
        ["consume-gate-persist-failed"],
        "successor must be stashed"
    );
}

#[test]
fn persist_and_stash_both_failing_still_never_raises() {
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    h.switcher.setup_directories().unwrap();
    let (security, armed) = backup_write_refuser(&h, "1", EMAIL);
    let stash_dir = h.switcher.credentials_dir.clone();
    let frozen = stash_dir.clone();
    let http = ScriptedHttp(move |_: &Request| {
        armed.store(true, Ordering::SeqCst);
        chmod(&frozen, 0o500);
        Ok(grant("sk-new", "rt-new"))
    });
    h.rebuild_with(security, Arc::new(http));
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    chmod(&stash_dir, 0o700);
    assert_eq!(
        out.error.as_deref(),
        Some("transient"),
        "reported success on a spent grant with nothing stashed"
    );
    assert_eq!(
        tokens(&out.credentials.unwrap()),
        ("sk-new".into(), "rt-new".into())
    );
    assert!(!out.stashed, "nothing reached the stash");
    assert!(h.stash().is_empty());
}

#[test]
fn a_failed_session_invalidation_does_not_discard_the_adoption_store_rejects() {
    // The store never advanced: claiming an adoption would return what the slot doesn't hold.
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    super::grant::stash_successor_of(&h, &new(), &old());
    let (security, armed) = backup_write_refuser(&h, "1", EMAIL);
    armed.store(true, Ordering::SeqCst);
    let http = h.sandbox.http.clone();
    h.rebuild_with(security, http);
    let adopted = h.switcher.adopt_stashed_successor("1", EMAIL, &old());
    assert!(adopted.is_err(), "the refused write must surface");
    assert_eq!(h.backup("1", EMAIL).unwrap(), old());
}

#[test]
fn backup_unreadable_after_post_is_not_a_removed_slot() {
    // The Keychain locked during the POST: unreadable is not removed, and the slot still holds
    // the spent generation, so the outcome is no success.
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let security = h.sandbox.security.clone();
    h.rebuild_with_http(Arc::new(ScriptedHttp(move |_: &Request| {
        security.answer_service(BACKUP_KEYCHAIN_SERVICE, Answer::Exit(36));
        Ok(grant("sk-new", "rt-new"))
    })));
    let out = h.switcher.consume_backup_grant("1", EMAIL, &old());
    h.sandbox.security.clear_service(BACKUP_KEYCHAIN_SERVICE);
    let reasons = reasons(&h);
    assert!(
        !reasons.is_empty(),
        "the consumed successor must still be stashed"
    );
    assert!(
        !reasons
            .iter()
            .any(|reason| reason == "consume-gate-slot-removed"),
        "an unreadable Keychain is not a removed slot"
    );
    assert!(
        out.error.is_some(),
        "`error: None` would tell callers it is safe to activate"
    );
}

#[test]
fn concurrent_gates_consume_only_one_grant() {
    let mut h = gate_harness();
    h.set_backup("1", EMAIL, &old());
    let (entered, in_post) = mpsc::channel::<()>();
    let (release, released) = mpsc::channel::<()>();
    let released = Mutex::new(released);
    let sent = Arc::new(Mutex::new(Vec::new()));
    let record = sent.clone();
    let http = Arc::new(ScriptedHttp(move |request: &Request| {
        if !request.url.starts_with(OAUTH_TOKEN_URL) {
            return Err(HttpError::Network("nothing queued".into()));
        }
        record.lock().unwrap().push(sent_refresh_token(request));
        entered.send(()).unwrap();
        released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        Ok(grant("sk-new", "rt-new"))
    }));
    let security = h.sandbox.security.clone();
    h.rebuild_with(security, http.clone());
    let mut env = h.sandbox.env();
    env.http = http;
    let mut second = Switcher::new(env);
    second.lock_timeout = Duration::from_millis(200);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| h.switcher.consume_backup_grant("1", EMAIL, &old()));
        in_post.recv_timeout(Duration::from_secs(5)).unwrap();
        let busy = second.consume_backup_grant("1", EMAIL, &old());
        assert_eq!(busy.error.as_deref(), Some("consume-busy"));
        release.send(()).unwrap();
        let first = first.join().unwrap();
        assert_eq!(first.error, None);
        assert_eq!(
            tokens(&first.credentials.unwrap()),
            ("sk-new".into(), "rt-new".into())
        );
    });
    let after = second.consume_backup_grant("1", EMAIL, &old());
    assert_eq!(after.error, None);
    assert_eq!(
        tokens(&after.credentials.unwrap()),
        ("sk-new".into(), "rt-new".into())
    );
    assert_eq!(
        *sent.lock().unwrap(),
        ["rt-old"],
        "one grant consumed exactly once"
    );
    assert_eq!(tokens(&h.backup("1", EMAIL).unwrap()).0, "sk-new");
}

//! cswap's `tests/test_switcher.py` (0fe22a8) `TestFetchAccountUsageSessionProfile`, the cases
//! of a backup served read-only under a live session: Koyo reads every idle backup that way
//! (`usage/idle.rs` says why), so they're its idle fetch, `_fetch_account_usage` and
//! `_read_only_fetch`, with no session. In `test_refused_credential_is_requested_once_across_
//! passes` the refused credential is the backup itself, cswap's being a session profile's.
//!
//! Skipped with session mode, the rest of the class: `test_fresh_session_credentials_fetch_
//! read_only`, `test_expired_session_credentials_with_live_session_is_sentinel`,
//! `test_rejected_session_credentials_with_live_session_is_sentinel`,
//! `test_stamped_session_credential_is_not_requested_again`,
//! `test_rotated_session_credential_after_a_stamp_is_requested`,
//! `test_expired_session_credentials_without_live_session_falls_back`,
//! `test_exited_session_ahead_of_backup_is_adopted`,
//! `test_live_session_ahead_of_backup_is_not_adopted`,
//! `test_unreadable_session_record_keeps_the_profile_read_only`,
//! `test_lock_contention_keeps_the_profile_read_only`, `test_failed_adoption_write_never_
//! raises`, `test_drifted_profile_email_falls_back_to_backup`,
//! `test_drifted_profile_org_same_email_falls_back`,
//! `test_matching_profile_identity_uses_session_credentials`,
//! `test_unreadable_profile_identity_trusts_session_credentials`; and, with the refresh inside
//! a usage fetch, `test_no_session_profile_uses_backup_path` and
//! `test_exited_session_rejected_backup_still_refreshes`.

use serde_json::json;

use super::Harness;
use super::support::{bearers, creds, info, posted, queue_usage, usage_requests};
use crate::Clock as _;
use crate::oauth::{USAGE_TOKEN_EXPIRED, USAGE_URL, access_token_fingerprint};
use crate::switcher::Fetch;
use crate::switcher::info::AccountInfo;

/// cswap's `_oauth_creds`: a token expiring `expires_in_s` from now.
fn expiring_in(h: &Harness, token: &str, expires_in_s: f64) -> String {
    let expires_ms = ((h.sandbox.clock.now() + expires_in_s) * 1000.).trunc();
    creds(token, &format!("rt-{token}"), json!(expires_ms))
}

/// cswap's `_info`: idle slot 2.
fn idle(backup: &str) -> AccountInfo {
    info("2", "test@example.com", "org-uuid", false, backup)
}

#[test]
fn live_session_without_profile_credentials_serves_backup_read_only() {
    let h = Harness::new();
    let backup = expiring_in(&h, "sk-backup", 7200.);
    queue_usage(&h, &json!({"five_hour": {"utilization": 7}}));
    let record = h.switcher.fetch_account_usage(&idle(&backup), None);
    assert_eq!(record.usage, Some(json!({"five_hour": {"pct": 7}})));
    assert_eq!(bearers(&h, USAGE_URL), ["sk-backup"]);
    assert!(posted(&h).is_empty(), "the read refreshed");
}

#[test]
fn live_session_without_profile_credentials_and_expired_backup_is_sentinel() {
    let h = Harness::new();
    let backup = expiring_in(&h, "sk-backup", -60.);
    let record = h.switcher.fetch_account_usage(&idle(&backup), None);
    assert_eq!(record.sentinel.as_deref(), Some(USAGE_TOKEN_EXPIRED));
    assert_eq!(usage_requests(&h), 0);
}

#[test]
fn live_session_rejected_backup_is_sentinel() {
    let h = Harness::new();
    let backup = expiring_in(&h, "sk-backup", 7200.);
    h.sandbox.http.queue_json(USAGE_URL, 401, &json!({}));
    let record = h.switcher.fetch_account_usage(&idle(&backup), None);
    assert_eq!(record.sentinel.as_deref(), Some(USAGE_TOKEN_EXPIRED));
    assert_eq!(record.error, None);
    assert_eq!(record.rejected_fp, access_token_fingerprint(&backup));
}

#[test]
fn stamped_backup_under_live_session_is_not_requested_again() {
    let h = Harness::new();
    let backup = expiring_in(&h, "sk-backup", 7200.);
    let stamp = access_token_fingerprint(&backup);
    let record = h
        .switcher
        .fetch_account_usage(&idle(&backup), stamp.as_deref());
    assert_eq!(record.sentinel.as_deref(), Some(USAGE_TOKEN_EXPIRED));
    assert_eq!(usage_requests(&h), 0);
}

#[test]
fn refused_credential_is_requested_once_across_passes() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    let backup = expiring_in(&h, "sk-backup", 7200.);
    h.sandbox.http.queue_json(USAGE_URL, 401, &json!({}));
    h.sandbox.http.queue_json(USAGE_URL, 401, &json!({}));
    let infos = [idle(&backup)];
    let first = h
        .switcher
        .collect_usage_entries(&infos, Fetch::Any)
        .unwrap()
        .remove("2")
        .unwrap();
    let second = h
        .switcher
        .collect_usage_entries(&infos, Fetch::Any)
        .unwrap()
        .remove("2")
        .unwrap();
    assert_eq!(first.sentinel.as_deref(), Some(USAGE_TOKEN_EXPIRED));
    assert_eq!(second.sentinel.as_deref(), Some(USAGE_TOKEN_EXPIRED));
    assert_eq!(
        usage_requests(&h),
        1,
        "a refused credential was asked again"
    );
}

#[test]
fn live_session_other_errors_keep_their_identity() {
    let h = Harness::new();
    let backup = expiring_in(&h, "sk-backup", 7200.);
    h.sandbox.http.queue(
        USAGE_URL,
        Ok(crate::http::Response {
            status: 429,
            headers: vec![("retry-after".into(), "3600".into())],
            body: b"{}".to_vec(),
        }),
    );
    let record = h.switcher.fetch_account_usage(&idle(&backup), None);
    assert_eq!(record.sentinel, None);
    assert_eq!(record.error.as_deref(), Some("http-429"));
    assert_eq!(record.retry_after_s, Some(3600.));
}

//! cswap's `tests/test_switcher.py` (0fe22a8) for the active account's fetch,
//! `_fetch_active_usage`: `TestActiveAccountRefresh`'s fast path, `TestDegradedReadProvenance`
//! and the active cases of `TestActiveSlotStrikeParity` and `TestUltraReviewCoverageGaps`. The
//! rotated-backup resync and its ownership probe are in `resync.rs`.
//!
//! Koyo never refreshes to read usage (`usage/active.rs` says why), so an expired active token
//! is the `token expired` sentinel. Adapted to that: `test_verdict_does_not_survive_a_slot_
//! identity_change` (no refresh, no write, no request either way), and
//! `test_active_path_refuses_consume_under_securestorage_env` (no POST; the sentinel, not
//! `store-unmirrored`, which only refuses a refresh Koyo never makes).
//!
//! Skipped, as the refresh of an expired or rejected active token under Claude Code's locks
//! isn't ported: `test_expired_refreshes_under_locks_and_persists_both_stores`,
//! `test_owner_present_no_longer_blocks_the_refresh`,
//! `test_lock_reread_adopts_a_fresher_live_credential`,
//! `test_lock_reread_never_adopts_a_wiped_live_credential`,
//! `test_non_oauth_live_with_moved_identity_is_never_clobbered`,
//! `test_empty_live_with_matching_identity_still_recovers`,
//! `test_a_held_consume_lock_defers_the_active_refresh`,
//! `test_filelock_contention_defers_instead_of_raising`,
//! `test_unattributed_live_recovers_from_the_slots_backup`,
//! `test_unattributed_live_with_unusable_backup_never_consumes`,
//! `test_live_read_error_defers_instead_of_consuming`,
//! `test_stranded_live_store_is_restored_from_the_backup`,
//! `test_identity_check_compares_organization_too`,
//! `test_locally_valid_but_server_rejected_token_refreshes`,
//! `test_server_rejected_token_lock_contention_still_surfaces_the_401`,
//! `test_no_refresh_token_is_permanent_not_transient`,
//! `test_unexpected_exception_defers_never_raises`,
//! `test_transient_refresh_failure_backs_off_via_the_store`,
//! `test_lock_timeout_defers_to_the_holder`,
//! `test_consumed_generation_survives_a_live_write_failure`,
//! `test_expiry_after_unresynced_rotation_defers_without_verified_lineage`,
//! `test_expiry_after_unresynced_rotation_consumes_live_when_verified`,
//! `test_expiry_after_tolerated_backup_write_failure_consumes_live`,
//! `test_stranded_backup_newer_still_restores_not_consumes_live`,
//! `test_backup_restore_keeps_the_live_shared_fields`,
//! `test_list_renders_token_expired_line_on_lock_contention`,
//! `test_foreign_live_credential_under_the_lock_is_never_consumed`,
//! `test_backup_write_failure_still_persists_live_and_never_raises`,
//! `test_dead_lineage_surfaces_invalid_grant_not_a_silent_sentinel`,
//! `test_adopting_a_cc_rotation_skips_the_resync_without_a_verdict`,
//! `test_adopting_a_cc_rotation_resyncs_the_slot_backup_when_verified`,
//! `test_adopting_a_known_foreign_credential_defers`; `TestActiveRefreshProvenance` (both);
//! `TestUltraReviewCoverageGaps`'s `test_demotion_downgrades_invalid_grant_when_source_moved`,
//! `test_demotion_reread_error_still_strikes`,
//! `test_active_invalid_grant_binds_strike_to_posted_input`; and, with session mode,
//! `test_add_account_refuses_inside_session_shell` and `test_add_token_refuses_inside_session_
//! shell` (the latter's API-key accounts too).

use serde_json::json;

use super::support::{
    EMAIL, creds, fresh, gate_harness, info, posted, queue_usage, usage_requests,
};
use super::{Harness, sample_sequence_data};
use crate::credentials::ActiveVerdict;
use crate::oauth::{
    OAUTH_TOKEN_URL, USAGE_KEYCHAIN_UNAVAILABLE, USAGE_NO_CREDENTIALS, USAGE_TOKEN_EXPIRED,
    USAGE_URL, credential_fingerprint,
};
use crate::paths::CLAUDE_CODE_KEYCHAIN_SERVICE;
use crate::testing::Answer;
use crate::usage_store::FetchRecord;

/// cswap's `_switcher(sample_sequence_data)` with `mock_claude_config`: slot 1 live.
pub(super) fn active_harness() -> Harness {
    let h = gate_harness();
    h.mock_claude_config();
    h
}

fn degraded() -> ActiveVerdict {
    ActiveVerdict {
        degraded: true,
        ..ActiveVerdict::default()
    }
}

/// cswap's `_fetch_active_usage(num, email, creds)` with the active read's `verdict`.
pub(super) fn active_fetch(
    h: &Harness,
    num: &str,
    email: &str,
    creds: &str,
    verdict: ActiveVerdict,
) -> FetchRecord {
    h.switcher
        .fetch_active_usage(&info(num, email, "", true, creds), verdict)
}

#[test]
fn non_expired_fetches_without_refreshing() {
    let h = active_harness();
    let live = fresh("sk-live", "rt-live");
    queue_usage(&h, &json!({"five_hour": {"utilization": 3}}));
    let result = active_fetch(&h, "1", EMAIL, &live, ActiveVerdict::default());
    assert_eq!(result.usage, Some(json!({"five_hour": {"pct": 3}})));
    assert!(posted(&h).is_empty());
    assert_eq!(usage_requests(&h), 1);
}

#[test]
fn verdict_does_not_survive_a_slot_identity_change() {
    let h = active_harness();
    let live_b = creds("sk-B", "rt-B", Some(json!(2000)));
    let key = h
        .switcher
        .lineage_key("1", EMAIL, &credential_fingerprint(&live_b).unwrap())
        .unwrap();
    h.switcher.set_probe_verdict(key, true);
    let mut data = h.sequence();
    data["accounts"]["1"]["uuid"] = "uuid-recreated".into();
    h.write_sequence(&data);
    h.set_backup("1", EMAIL, &creds("sk-A", "rt-A", Some(json!(1000))));
    h.set_live(&live_b);
    let result = active_fetch(&h, "1", EMAIL, &live_b, ActiveVerdict::default());
    assert_eq!(result.sentinel.as_deref(), Some(USAGE_TOKEN_EXPIRED));
    assert!(posted(&h).is_empty());
    assert_eq!(h.live().unwrap(), live_b, "the live store was written");
    assert_eq!(usage_requests(&h), 0);
}

#[test]
fn no_token_returns_no_credentials() {
    let h = active_harness();
    let result = active_fetch(&h, "1", EMAIL, "", ActiveVerdict::default());
    assert_eq!(result.sentinel.as_deref(), Some(USAGE_NO_CREDENTIALS));
    assert_eq!(usage_requests(&h), 0);
}

#[test]
fn degraded_active_read_never_consumes() {
    // A degraded read may be a superseded generation: it is never spent, and says why.
    let h = active_harness();
    let stale = creds("sk-stale", "rt-stale", Some(json!(1000)));
    h.set_backup("1", EMAIL, &stale);
    let result = active_fetch(&h, "1", EMAIL, &stale, degraded());
    assert!(posted(&h).is_empty());
    assert_eq!(result.sentinel.as_deref(), Some(USAGE_KEYCHAIN_UNAVAILABLE));
    assert_eq!(result.error, None);
    assert_eq!(usage_requests(&h), 0);
}

#[test]
fn status_path_sets_the_degraded_flag_too() {
    // cswap's status path read the active credential without recording the verdict; here
    // every path builds the accounts info, whose active row carries it.
    let h = active_harness();
    let stale = creds("sk-stale", "rt-stale", Some(json!(1000)));
    std::fs::write(h.credentials_file(), &stale).unwrap();
    h.sandbox
        .security
        .answer_service(CLAUDE_CODE_KEYCHAIN_SERVICE, Answer::Exit(36));
    let infos = h.switcher.build_accounts_info().unwrap();
    let active = infos.iter().find(|info| info.is_active()).unwrap();
    assert_eq!(
        active.creds, stale,
        "premise: the file covered the failed Keychain read"
    );
    assert!(
        active.active.unwrap().degraded,
        "the degraded guard is disarmed"
    );
    assert_eq!(infos.iter().filter(|info| info.is_active()).count(), 1);
}

#[test]
fn degraded_plus_server_401_defers_with_the_401_record() {
    let h = active_harness();
    let live = fresh("sk-f", "rt-f");
    h.sandbox.http.queue_json(USAGE_URL, 401, &json!({}));
    let result = active_fetch(&h, "1", EMAIL, &live, degraded());
    assert!(posted(&h).is_empty());
    assert_eq!(result.error.as_deref(), Some("http-401"));
    assert_eq!(result.sentinel, None);
}

#[test]
fn server_rejected_token_with_no_recovery_surfaces_the_401() {
    // A locally valid token the server refused is a failure record, never "token expired".
    let h = active_harness();
    let revoked = fresh("sk-revoked", "rt-foreign");
    h.set_live(&revoked);
    h.set_backup(
        "1",
        EMAIL,
        &json!({"claudeAiOauth": {"accessToken": "", "refreshToken": ""}}).to_string(),
    );
    h.sandbox.http.queue_json(USAGE_URL, 401, &json!({}));
    let result = active_fetch(&h, "1", EMAIL, &revoked, ActiveVerdict::default());
    assert_eq!(result.error.as_deref(), Some("http-401"));
    assert_eq!(result.sentinel, None);
    assert!(posted(&h).is_empty());
}

#[test]
fn degraded_read_never_feeds_resync_write() {
    let h = Harness::new();
    let mut data = sample_sequence_data();
    data["accounts"]["2"]["email"] = "b@example.com".into();
    h.write_sequence(&data);
    h.mock_claude_config();
    let live = fresh("sk-a", "rt-a");
    h.set_live(&live);
    h.queue_profile(&super::profile("uuid-2", Some("b@example.com"), Some("")));
    queue_usage(&h, &json!({"five_hour": {"utilization": 10}}));
    active_fetch(&h, "2", "b@example.com", &live, degraded());
    assert_eq!(h.profile_lookups(), 0, "the resync ran on a degraded read");
    assert_eq!(h.backup("2", "b@example.com"), None);
}

#[test]
fn active_path_refuses_consume_under_securestorage_env() {
    let mut h = Harness::new();
    h.sandbox.vars.securestorage_config_dir = Some("/elsewhere/redir".into());
    h.rebuild();
    let mut data = sample_sequence_data();
    data["accounts"]["2"]["email"] = "b@example.com".into();
    h.write_sequence(&data);
    h.mock_claude_config();
    let expired = creds("sk-exp", "rt-exp", Some(json!(1000)));
    h.set_backup("2", "b@example.com", &expired);
    let result = active_fetch(&h, "2", "b@example.com", &expired, ActiveVerdict::default());
    assert!(h.sandbox.http.requests_to(OAUTH_TOKEN_URL).is_empty());
    assert_eq!(result.sentinel.as_deref(), Some(USAGE_TOKEN_EXPIRED));
}

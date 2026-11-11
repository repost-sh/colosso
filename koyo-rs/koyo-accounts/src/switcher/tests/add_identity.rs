//! cswap's `tests/test_add_account_identity.py` (0fe22a8): `add_account` must not store a
//! credential that belongs to another account. cswap's tests add into `--slot 7`; Koyo's add
//! takes the next slot, so "slot 7 wasn't created" reads "no slot was created".
//! `test_the_guard_receives_the_triple_THAT_WAS_READ_not_a_rebuild` is left out: it checks
//! Python object identity (`is`), which a Rust value has no counterpart of.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{Harness, ScriptedHttp, profile};
use crate::AccountsError;
use crate::oauth::{OAUTH_TOKEN_URL, PROFILE_URL, extract_oauth_data};
use crate::paths::CLAUDE_CODE_KEYCHAIN_SERVICE;
use crate::testing::json_response;

fn creds_theirs() -> String {
    json!({"claudeAiOauth": {
        "accessToken": "sk-ant-oat01-THEIRS", "refreshToken": "rt-theirs",
        "expiresAt": 99_999_999_999_000_u64,
    }})
    .to_string()
}

/// cswap's `_switcher`: a config naming `email` in `org`, the live credential `creds` (the
/// store is set up by the add).
fn switcher(email: &str, org: &str, creds: &str) -> Harness {
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {"emailAddress": email, "organizationUuid": org}}));
    h.set_live(creds);
    h
}

fn added(h: &Harness) -> bool {
    !h.accounts().is_empty()
}

fn refusal(h: &Harness) -> String {
    match h.switcher.add_account() {
        Err(AccountsError::Config(why) | AccountsError::Validation(why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn add_refuses_a_credential_whose_owner_is_a_different_account() {
    for foreign_org in [Some(""), None] {
        let h = switcher("ax@example.com", "", &creds_theirs());
        h.queue_profile(&profile("u-other", Some("other@example.com"), foreign_org));
        let why = refusal(&h).to_lowercase();
        assert!(
            why.contains("other@example.com")
                || why.contains("does not")
                || why.contains("mismatch"),
            "the refusal must name the conflict: {why}"
        );
        assert!(
            !added(&h),
            "a slot labelled ax@ holding another's credential"
        );
    }
}

#[test]
fn control_add_accepts_when_the_token_is_the_claimed_account() {
    let h = switcher("ax@example.com", "", &creds_theirs());
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), Some("")));
    h.switcher.add_account().unwrap();
    assert_eq!(h.accounts()["1"]["email"], "ax@example.com");
}

#[test]
fn control_unresolvable_profile_still_registers() {
    let h = switcher("ax@example.com", "", &creds_theirs());
    h.switcher.add_account().unwrap();
    assert_eq!(h.accounts()["1"]["email"], "ax@example.com");
}

#[test]
fn expired_token_with_no_refresh_token_skips_the_profile_fetch() {
    let expired = json!({"claudeAiOauth": {"accessToken": "sk-ant-oat01-EXPIRED", "expiresAt": 1}});
    let h = switcher("ax@example.com", "", &expired.to_string());
    h.switcher.add_account().unwrap();
    assert_eq!(h.profile_lookups(), 0);
    assert_eq!(h.accounts()["1"]["email"], "ax@example.com");
}

#[test]
fn control_expired_credential_whose_refresh_fails_still_registers() {
    let expired = json!({"claudeAiOauth": {
        "accessToken": "sk-ant-oat01-STALE", "refreshToken": "rt-dead", "expiresAt": 1,
    }});
    let h = switcher("ax@example.com", "", &expired.to_string());
    h.switcher.add_account().unwrap();
    assert_eq!(h.profile_lookups(), 0);
    assert_eq!(h.accounts()["1"]["email"], "ax@example.com");
}

#[test]
fn same_email_different_org_is_still_a_mismatch() {
    let h = switcher("ax@example.com", "org-A", &creds_theirs());
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), Some("org-B")));
    refusal(&h);
    assert!(!added(&h), "a token from another organisation was accepted");
}

fn own(tag: &str) -> String {
    json!({"claudeAiOauth": {
        "accessToken": format!("sk-ant-oat01-{tag}"), "refreshToken": format!("rt-{tag}"),
        "expiresAt": 99_999_999_999_000_u64,
    }})
    .to_string()
}

#[test]
fn add_refuses_a_foreign_credential_on_refresh_in_place() {
    for foreign_org in [Some(""), None] {
        let h = switcher("ax@example.com", "", &own("MINE"));
        // One answer per lookup: the first add's, then the foreign one.
        let answers = [
            profile("u-ax", Some("ax@example.com"), Some("")),
            profile("u-other", Some("other@example.com"), foreign_org),
        ];
        for answer in &answers {
            h.sandbox.http.queue_json(PROFILE_URL, 200, answer);
        }
        h.switcher.add_account().unwrap();
        h.set_live(&creds_theirs());
        refusal(&h);
        let stored = h.backup("1", "ax@example.com").unwrap();
        assert!(
            !stored.contains("THEIRS"),
            "the slot's label now holds another's token"
        );
    }
}

#[test]
fn control_refresh_in_place_still_accepts_the_owning_token() {
    let h = switcher("ax@example.com", "", &own("MINE"));
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), Some("")));
    h.switcher.add_account().unwrap();
    h.set_live(&own("MINE-ROTATED"));
    assert!(h.switcher.add_account().unwrap().updated);
    assert!(
        h.backup("1", "ax@example.com")
            .unwrap()
            .contains("MINE-ROTATED")
    );
}

#[test]
fn org_slot_registers_when_the_resolved_profile_has_no_org() {
    // No organisation block is unverifiable about the org, never a refusal.
    let h = switcher("ax@example.com", "org-A", &creds_theirs());
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), None));
    h.switcher.add_account().unwrap();
    assert_eq!(h.accounts()["1"]["email"], "ax@example.com");
}

#[test]
fn control_org_slot_still_registers_when_the_org_matches() {
    let h = switcher("ax@example.com", "org-A", &creds_theirs());
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), Some("org-A")));
    h.switcher.add_account().unwrap();
    assert_eq!(h.accounts()["1"]["email"], "ax@example.com");
}

#[test]
fn control_org_slot_still_refuses_a_different_email() {
    let h = switcher("ax@example.com", "org-A", &creds_theirs());
    h.queue_profile(&profile("u-o", Some("other@example.com"), Some("org-A")));
    refusal(&h);
    assert!(!added(&h));
}

#[test]
fn org_mismatch_message_names_both_organizations() {
    let h = switcher("ax@example.com", "org-A", &creds_theirs());
    h.queue_profile(&profile("u-ax2", Some("ax@example.com"), Some("org-B")));
    let why = refusal(&h);
    assert!(why.contains("org-A") && why.contains("org-B"), "{why}");
}

#[test]
fn email_mismatch_message_still_names_both_addresses() {
    let h = switcher("ax@example.com", "", &creds_theirs());
    h.queue_profile(&profile("u-other", Some("other@example.com"), Some("")));
    let why = refusal(&h);
    assert!(
        why.contains("ax@example.com") && why.contains("other@example.com"),
        "{why}"
    );
}

#[test]
fn control_personal_account_empty_org_still_registers() {
    let h = switcher("ax@example.com", "", &creds_theirs());
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), None));
    h.switcher.add_account().unwrap();
    assert_eq!(h.accounts()["1"]["email"], "ax@example.com");
}

#[test]
fn an_expired_credential_is_unresolvable_and_never_consumes_a_grant() {
    let expired = json!({"claudeAiOauth": {
        "accessToken": "sk-ant-oat01-STALE", "refreshToken": "rt-live", "expiresAt": 1,
    }});
    let h = switcher("ax@example.com", "", &expired.to_string());
    h.switcher.add_account().unwrap();
    assert!(h.sandbox.http.requests_to(OAUTH_TOKEN_URL).is_empty());
    assert_eq!(h.profile_lookups(), 0);
    assert!(added(&h));
}

fn with_uuid(email: &str, uuid: &Value) -> Harness {
    let h = Harness::new();
    h.write_config(&json!({"oauthAccount": {
        "emailAddress": email, "organizationUuid": "", "accountUuid": uuid,
    }}));
    h.set_live(&creds_theirs());
    h
}

#[test]
fn a_matching_uuid_accepts_even_when_the_email_changed() {
    let h = with_uuid("old@example.com", &"u-same".into());
    h.queue_profile(&profile("u-same", Some("new@example.com"), Some("")));
    h.switcher.add_account().unwrap();
    assert!(added(&h));
}

#[test]
fn a_recycled_email_under_a_different_uuid_is_refused() {
    let h = with_uuid("ax@example.com", &"u-registered".into());
    h.queue_profile(&profile("u-recreated", Some("ax@example.com"), Some("")));
    refusal(&h);
    assert!(!added(&h));
}

#[test]
fn an_expired_foreign_credential_registers_and_that_is_deliberate() {
    let expired = json!({"claudeAiOauth": {
        "accessToken": "sk-ant-oat01-THEIRS-STALE", "refreshToken": "rt-theirs", "expiresAt": 1,
    }});
    let h = switcher("ax@example.com", "", &expired.to_string());
    h.switcher.add_account().unwrap();
    // No grant moved.
    assert!(h.sandbox.http.requests_to(OAUTH_TOKEN_URL).is_empty());
    assert_eq!(h.profile_lookups(), 0);
    assert!(added(&h));
}

#[test]
fn an_unverifiable_ownership_check_says_so_out_loud() {
    let h = switcher("ax@example.com", "", &creds_theirs());
    let result = h.switcher.add_account().unwrap();
    assert!(added(&h), "premise: it must still register");
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.to_lowercase().contains("could not")),
        "fail-open was silent: {:?}",
        result.notices
    );
}

/// A switcher whose profile lookup runs `during` and answers `answer`.
fn with_lookup(h: &mut Harness, answer: Value, during: impl Fn() + Send + Sync + 'static) {
    h.rebuild_with_http(Arc::new(ScriptedHttp(move |_: &crate::http::Request| {
        during();
        Ok(json_response(200, &answer))
    })));
}

#[test]
fn a_login_landing_during_the_guards_network_window_is_refused() {
    let mut h = switcher("ax@example.com", "", &creds_theirs());
    let config = h.config_path();
    with_lookup(
        &mut h,
        profile("u-ax", Some("ax@example.com"), Some("")),
        move || {
            std::fs::write(
                &config,
                json!({"oauthAccount": {
                    "emailAddress": "someone-else@example.com", "organizationUuid": "",
                    "accountUuid": "u-someone-else",
                }})
                .to_string(),
            )
            .unwrap();
        },
    );
    refusal(&h);
    assert!(!added(&h));
}

fn tagged(tag: &str) -> String {
    json!({"claudeAiOauth": {
        "accessToken": format!("sk-ant-oat01-{tag}"),
        "refreshToken": format!("sk-ant-ort01-{tag}"),
        "expiresAt": 9_999_999_999_999_u64,
    }})
    .to_string()
}

#[test]
fn a_refresh_landing_in_the_guards_window_is_refused() {
    let mut h = switcher("ax@example.com", "", &tagged("OLD"));
    let security = h.sandbox.security.clone();
    let user = h.user();
    with_lookup(
        &mut h,
        profile("u-ax", Some("ax@example.com"), Some("")),
        move || {
            security.set(CLAUDE_CODE_KEYCHAIN_SERVICE, &user, &tagged("NEW"));
        },
    );
    refusal(&h);
    assert!(!added(&h));
}

#[test]
fn a_matching_uuid_does_not_excuse_a_different_org() {
    let h = with_uuid("ax@example.com", &"u-ax".into());
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), Some("org-X")));
    refusal(&h);
    assert!(!added(&h));
}

#[test]
fn a_null_account_uuid_does_not_trip_the_commit_time_recheck() {
    let h = with_uuid("ax@example.com", &Value::Null);
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), Some("")));
    h.switcher.add_account().unwrap();
    // And the null never reaches the roster.
    assert_eq!(h.accounts()["1"]["uuid"], "");
}

#[test]
fn the_refresh_in_place_path_also_refuses_a_login_in_the_window() {
    let mut h = switcher("ax@example.com", "", &creds_theirs());
    h.queue_profile(&profile("u-ax", Some("ax@example.com"), Some("")));
    h.switcher.add_account().unwrap();
    assert!(added(&h), "premise: registered");
    let config = h.config_path();
    with_lookup(
        &mut h,
        profile("u-ax", Some("ax@example.com"), Some("")),
        move || {
            std::fs::write(
                &config,
                json!({"oauthAccount": {
                    "emailAddress": "someone-else@example.com", "organizationUuid": "",
                    "accountUuid": "u-other",
                }})
                .to_string(),
            )
            .unwrap();
        },
    );
    refusal(&h);
    let stored: Value = serde_json::from_str(&h.config_backup("1", "ax@example.com")).unwrap();
    assert_eq!(stored["oauthAccount"]["emailAddress"], "ax@example.com");
}

#[test]
fn add_refuses_a_wiped_credential() {
    let wiped = json!({"claudeAiOauth": {"accessToken": "", "refreshToken": "", "expiresAt": 0}});
    let h = switcher("ax@example.com", "", &wiped.to_string());
    let Err(AccountsError::Config(why)) = h.switcher.add_account() else {
        panic!("a wiped credential must be refused");
    };
    assert!(why.contains("Nothing was changed"), "{why}");
    assert_eq!(h.profile_lookups(), 0);
    assert!(!added(&h));
}

#[test]
fn control_refresh_token_without_access_token_still_registers() {
    let creds =
        json!({"claudeAiOauth": {"accessToken": "", "refreshToken": "rt-live", "expiresAt": 0}});
    let h = switcher("ax@example.com", "", &creds.to_string());
    h.switcher.add_account().unwrap();
    assert!(added(&h));
}

#[test]
fn extract_oauth_data_is_none_for_json_that_is_not_an_object() {
    assert_eq!(extract_oauth_data("[]"), None);
    assert_eq!(extract_oauth_data(r#""x""#), None);
}

#[test]
fn a_config_torn_mid_rewrite_in_that_window_is_refused_not_a_crash() {
    let mut h = switcher("ax@example.com", "", &creds_theirs());
    let config = h.config_path();
    with_lookup(
        &mut h,
        profile("u-ax", Some("ax@example.com"), Some("")),
        move || {
            std::fs::write(&config, r#"{"oauthAccount": {"emailAddr"#).unwrap();
        },
    );
    assert!(h.switcher.add_account().is_err());
    assert!(!added(&h));
}

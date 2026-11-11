//! cswap's `tests/test_session.py` (0fe22a8) `TestCaptureCredentials` (`add_account` under
//! `CLAUDE_CONFIG_DIR` and `CLAUDE_SECURESTORAGE_CONFIG_DIR`), on macOS, the one platform Koyo
//! has, and the profile-read cases its port rests on (`test_symlinked_profile_…`,
//! `test_relative_symlink_…`, `test_lexical_target_…`, `test_raw_joined_target_…`,
//! `test_session_read_still_falls_back_on_keychain_error`), driven through
//! `read_config_dir_credentials` as cswap's `read_session_credentials` drives it.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::{Harness, access_creds};
use crate::AccountsError;
use crate::paths::{CLAUDE_CODE_KEYCHAIN_SERVICE, keychain_service_name};
use crate::testing::Answer;

const ACTIVE_TOKEN: &str = "active-store-token";
const CONFIG_DIR_TOKEN: &str = "config-dir-token";

fn config_dir_config() -> String {
    json!({"oauthAccount": {
        "emailAddress": "elsewhere@example.com", "accountUuid": "uuid-elsewhere",
        "organizationUuid": "org-elsewhere",
    }})
    .to_string()
}

/// cswap's `_config_dir`: a profile with its identity and, unless `None`, its seed.
fn make_config_dir(h: &Harness, credentials: Option<&str>) -> PathBuf {
    let dir = h.sandbox.root().join("elsewhere");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".claude.json"), config_dir_config()).unwrap();
    if let Some(credentials) = credentials {
        std::fs::write(dir.join(".credentials.json"), credentials).unwrap();
    }
    dir
}

/// cswap's `_secure_dir`: credentials only, no identity.
fn make_secure_dir(h: &Harness) -> PathBuf {
    let dir = h.sandbox.root().join("securestore");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(".credentials.json"),
        access_creds("secure-store-token"),
    )
    .unwrap();
    dir
}

fn path_str(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

/// A switcher with these variables (the store is set up by the add).
fn with_vars(mut h: Harness, config_dir: Option<String>, secure: Option<String>) -> Harness {
    h.sandbox.vars.claude_config_dir = config_dir;
    h.sandbox.vars.securestorage_config_dir = secure;
    h.rebuild();
    h
}

fn stored(h: &Harness) -> String {
    h.backup("1", "elsewhere@example.com").unwrap_or_default()
}

fn not_added(h: &Harness) -> bool {
    !h.accounts().contains_key("1")
}

#[test]
fn captures_config_dir_token() {
    let h = Harness::new();
    h.set_live(&access_creds(ACTIVE_TOKEN));
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let h = with_vars(h, Some(path_str(&dir)), None);
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains(CONFIG_DIR_TOKEN));
    assert!(!stored(&h).contains(ACTIVE_TOKEN));
}

#[test]
fn macos_prefers_hashed_keychain_entry() {
    let h = Harness::new();
    h.set_live(&access_creds(ACTIVE_TOKEN));
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    h.sandbox.security.set(
        &keychain_service_name(&path_str(&dir)),
        &h.user(),
        &access_creds("rotated"),
    );
    let h = with_vars(h, Some(path_str(&dir)), None);
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains("rotated"));
}

#[test]
fn trailing_slash_still_finds_keychain_entry() {
    // Claude Code hashes the exported string verbatim.
    let h = Harness::new();
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let exported = format!("{}/", path_str(&dir));
    h.sandbox.security.set(
        &keychain_service_name(&exported),
        &h.user(),
        &access_creds("rotated"),
    );
    let h = with_vars(h, Some(exported), None);
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains("rotated"));
    assert!(!stored(&h).contains(CONFIG_DIR_TOKEN));
}

#[test]
fn default_config_dir_uses_active_store() {
    let h = Harness::new();
    h.set_live(&access_creds(ACTIVE_TOKEN));
    let default = path_str(&h.home().join(".claude"));
    let h = with_vars(h, Some(default), None);
    std::fs::write(h.switcher.claude_config_path(), config_dir_config()).unwrap();
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains(ACTIVE_TOKEN));
}

#[test]
fn symlinked_default_config_dir_uses_active_store() {
    let h = Harness::new();
    h.set_live(&access_creds(ACTIVE_TOKEN));
    let link = h.sandbox.root().join("home-link");
    std::os::unix::fs::symlink(h.home(), &link).unwrap();
    let h = with_vars(h, Some(path_str(&link.join(".claude"))), None);
    std::fs::write(h.switcher.claude_config_path(), config_dir_config()).unwrap();
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains(ACTIVE_TOKEN));
}

#[test]
fn api_key_login_still_reaches_guard() {
    let h = Harness::new();
    let dir = make_config_dir(&h, None);
    std::fs::write(
        dir.join(".claude.json"),
        json!({
            "oauthAccount": {"emailAddress": "elsewhere@example.com"},
            "primaryApiKey": format!("sk-ant-api03-{}", "x".repeat(20)),
        })
        .to_string(),
    )
    .unwrap();
    let h = with_vars(h, Some(path_str(&dir)), None);
    let Err(AccountsError::Validation(why)) = h.switcher.add_account() else {
        panic!("an API-key login must be refused");
    };
    assert!(why.contains("API-key account"), "{why}");
}

#[test]
fn machine_managed_key_does_not_answer_for_config_dir() {
    let h = Harness::new();
    h.sandbox.security.set(
        "Claude Code",
        &h.user(),
        &format!("sk-ant-api03-{}", "x".repeat(20)),
    );
    let dir = make_config_dir(&h, None);
    let h = with_vars(h, Some(path_str(&dir)), None);
    assert!(matches!(
        h.switcher.add_account(),
        Err(AccountsError::CredentialRead(_))
    ));
}

#[test]
fn credentialless_config_dir_does_not_fall_back() {
    let h = Harness::new();
    h.set_live(&access_creds(ACTIVE_TOKEN));
    let dir = make_config_dir(&h, None);
    let h = with_vars(h, Some(path_str(&dir)), None);
    assert!(matches!(
        h.switcher.add_account(),
        Err(AccountsError::CredentialRead(_))
    ));
}

#[test]
fn in_place_refresh_uses_same_source() {
    let h = Harness::new();
    h.set_live(&access_creds(ACTIVE_TOKEN));
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let h = with_vars(h, Some(path_str(&dir)), None);
    h.switcher.add_account().unwrap();
    std::fs::write(dir.join(".credentials.json"), access_creds("rotated")).unwrap();
    assert!(h.switcher.add_account().unwrap().updated);
    assert!(stored(&h).contains("rotated"));
}

#[test]
fn no_config_dir_uses_active_store() {
    for value in [None, Some(String::new())] {
        let h = Harness::new();
        let h = with_vars(h, value, None);
        h.set_live(&access_creds(ACTIVE_TOKEN));
        std::fs::write(h.switcher.claude_config_path(), config_dir_config()).unwrap();
        h.switcher.add_account().unwrap();
        assert!(stored(&h).contains(ACTIVE_TOKEN));
    }
}

#[test]
fn unreadable_keychain_fails_closed() {
    // One bounded retry, then the add fails rather than capture the seed.
    let h = Harness::new();
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let service = keychain_service_name(&path_str(&dir));
    h.sandbox
        .security
        .answer_service(&service, Answer::Exit(51));
    let h = with_vars(h, Some(path_str(&dir)), None);
    h.sandbox.security.clear_calls();
    let Err(AccountsError::CredentialRead(why)) = h.switcher.add_account() else {
        panic!("an unreadable Keychain must fail the add");
    };
    assert!(why.contains("unreadable"), "{why}");
    let calls = h
        .sandbox
        .security
        .calls()
        .iter()
        .filter(|call| call.args.contains(&service))
        .count();
    assert_eq!(calls, 2);
    assert!(not_added(&h));
}

#[test]
fn transient_keychain_error_retries() {
    let h = Harness::new();
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    h.sandbox.security.set(
        &keychain_service_name(&path_str(&dir)),
        &h.user(),
        &access_creds("rotated"),
    );
    let h = with_vars(h, Some(path_str(&dir)), None);
    // The capture's read is the add's first `security` call.
    h.sandbox.security.answer_next(Answer::Exit(51));
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains("rotated"));
}

#[test]
fn session_read_still_falls_back_on_keychain_error() {
    // The best-effort read stays best-effort: a locked Keychain reads the seed. Only capture
    // is strict.
    let h = Harness::new();
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    h.sandbox.security.answer_all(Answer::Exit(51));
    let read = h
        .switcher
        .read_config_dir_credentials(&path_str(&dir), false, None)
        .unwrap();
    assert!(read.unwrap().contains(CONFIG_DIR_TOKEN));
}

#[test]
fn securestorage_dir_overrides_config_dir() {
    let h = Harness::new();
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let secure = make_secure_dir(&h);
    let h = with_vars(h, Some(path_str(&dir)), Some(path_str(&secure)));
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains("secure-store-token"));
    assert!(!stored(&h).contains(CONFIG_DIR_TOKEN));
}

#[test]
fn securestorage_hashed_keychain_entry() {
    let h = Harness::new();
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let secure = make_secure_dir(&h);
    h.sandbox.security.set(
        &keychain_service_name(&path_str(&secure)),
        &h.user(),
        &access_creds("rotated"),
    );
    let h = with_vars(h, Some(path_str(&dir)), Some(path_str(&secure)));
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains("rotated"));
}

#[test]
fn empty_securestorage_dir_forces_default_store() {
    let h = Harness::new();
    h.set_live(&access_creds(ACTIVE_TOKEN));
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let h = with_vars(h, Some(path_str(&dir)), Some(String::new()));
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains(ACTIVE_TOKEN));
    assert!(!stored(&h).contains(CONFIG_DIR_TOKEN));
}

#[test]
fn securestorage_without_config_dir() {
    let h = Harness::new();
    let secure = make_secure_dir(&h);
    let h = with_vars(h, None, Some(path_str(&secure)));
    std::fs::write(h.switcher.claude_config_path(), config_dir_config()).unwrap();
    h.switcher.add_account().unwrap();
    assert!(stored(&h).contains("secure-store-token"));
}

#[test]
fn empty_selected_store_does_not_leak_config_profile() {
    let h = Harness::new();
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let h = with_vars(h, Some(path_str(&dir)), Some(String::new()));
    assert!(matches!(
        h.switcher.add_account(),
        Err(AccountsError::CredentialRead(_))
    ));
    assert!(not_added(&h));
}

#[test]
fn credentialless_securestorage_default_dir_does_not_fall_back() {
    // A non-empty override naming `~/.claude` uses the hashed name: neither the unsuffixed
    // item nor the config profile's seed may answer.
    let h = Harness::new();
    h.set_live(&access_creds(ACTIVE_TOKEN));
    assert!(
        h.sandbox
            .security
            .get(CLAUDE_CODE_KEYCHAIN_SERVICE, &h.user())
            .is_some()
    );
    let dir = make_config_dir(&h, Some(&access_creds(CONFIG_DIR_TOKEN)));
    let default = path_str(&h.home().join(".claude"));
    let h = with_vars(h, Some(path_str(&dir)), Some(default));
    assert!(matches!(
        h.switcher.add_account(),
        Err(AccountsError::CredentialRead(_))
    ));
    assert!(not_added(&h));
}

fn read_profile(h: &Harness, dir: &Path) -> String {
    h.switcher
        .read_config_dir_credentials(&path_str(dir), false, None)
        .unwrap()
        .unwrap_or_default()
}

#[test]
fn symlinked_profile_reads_the_target_keychain_entry() {
    let h = Harness::new();
    let target = h.sandbox.root().join("space");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(
        target.join(".credentials.json"),
        access_creds("sk-stale-seed"),
    )
    .unwrap();
    let session = h.sandbox.root().join("sess");
    std::os::unix::fs::symlink(&target, &session).unwrap();
    h.sandbox.security.set(
        &keychain_service_name(&path_str(&target)),
        &h.user(),
        &access_creds("sk-rotated"),
    );
    assert!(read_profile(&h, &session).contains("sk-rotated"));
}

#[test]
fn symlinked_profile_prefers_its_own_keychain_entry() {
    let h = Harness::new();
    let target = h.sandbox.root().join("space");
    std::fs::create_dir_all(&target).unwrap();
    let session = h.sandbox.root().join("sess");
    std::os::unix::fs::symlink(&target, &session).unwrap();
    h.sandbox.security.set(
        &keychain_service_name(&path_str(&target)),
        &h.user(),
        &access_creds("sk-target"),
    );
    h.sandbox.security.set(
        &keychain_service_name(&path_str(&session)),
        &h.user(),
        &access_creds("sk-own"),
    );
    assert!(read_profile(&h, &session).contains("sk-own"));
}

#[test]
fn relative_symlink_target_is_normalised_before_hashing() {
    let h = Harness::new();
    let root = h.sandbox.root();
    let target = root.join("space");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(
        target.join(".credentials.json"),
        access_creds("sk-stale-seed"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    let session = root.join("sessions/profile");
    std::os::unix::fs::symlink("../space", &session).unwrap();
    h.sandbox.security.set(
        &keychain_service_name(&path_str(&target)),
        &h.user(),
        &access_creds("sk-rotated"),
    );
    assert!(read_profile(&h, &session).contains("sk-rotated"));
}

#[test]
fn lexical_target_naming_another_directory_is_not_consulted() {
    // Under a parent that is itself a symlink elsewhere, `..` resolves under the parent's
    // target, so the lexical spelling names an unrelated folder.
    let h = Harness::new();
    let root = h.sandbox.root();
    std::fs::create_dir_all(root.join("leases/sessions")).unwrap();
    std::fs::create_dir_all(root.join("leases/space")).unwrap();
    std::fs::write(
        root.join("leases/space/.credentials.json"),
        access_creds("sk-seed"),
    )
    .unwrap();
    std::os::unix::fs::symlink("leases/sessions", root.join("sessions")).unwrap();
    let session = root.join("sessions/profile");
    std::os::unix::fs::symlink("../space", &session).unwrap();
    let unrelated = root.join("space");
    std::fs::create_dir_all(&unrelated).unwrap();
    h.sandbox.security.set(
        &keychain_service_name(&path_str(&unrelated)),
        &h.user(),
        &access_creds("sk-unrelated"),
    );
    assert!(read_profile(&h, &session).contains("sk-seed"));
}

#[test]
fn raw_joined_target_spelling_still_matches() {
    let h = Harness::new();
    let root = h.sandbox.root();
    let target = root.join("space");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(
        target.join(".credentials.json"),
        access_creds("sk-stale-seed"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("sessions")).unwrap();
    let session = root.join("sessions/profile");
    std::os::unix::fs::symlink("../space", &session).unwrap();
    let raw = format!("{}/../space", path_str(&root.join("sessions")));
    h.sandbox.security.set(
        &keychain_service_name(&raw),
        &h.user(),
        &access_creds("sk-rotated"),
    );
    assert!(read_profile(&h, &session).contains("sk-rotated"));
}

#[test]
fn normpath_matches_python() {
    use crate::paths::normpath;
    for (path, expected) in [
        ("/x/sessions/../space", "/x/space"),
        ("a/./b//c/", "a/b/c"),
        ("../a", "../a"),
        ("/../a", "/a"),
        ("//a", "//a"),
        ("///a", "/a"),
        ("", "."),
        ("a/..", "."),
    ] {
        assert_eq!(normpath(path), expected, "{path}");
    }
}

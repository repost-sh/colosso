//! `test_credentials.py` (the active read stays on one profile, the secure-storage override),
//! and the active-store cases of `test_switcher.py`'s `TestMacosKeychainFallback` and
//! `TestDegradedReadProvenance`.

use std::fs;
use std::time::{Duration, SystemTime};

use super::{World, cred};
use crate::AccountsError;
use crate::fsutil;
use crate::paths::{self, CLAUDE_CODE_KEYCHAIN_SERVICE, keychain_service_name};
use crate::testing::Answer;

const MANAGED: &str = "Claude Code";

fn default_creds() -> String {
    cred("default-profile")
}

fn custom_creds() -> String {
    cred("custom-profile")
}

/// A world whose `CLAUDE_CONFIG_DIR` (and secure-storage folder) are made under the sandbox.
fn with_config_dir(name: &str, secure: Option<&str>) -> (World, String) {
    let mut dir = String::new();
    let world = World::with(|sandbox| {
        let custom = sandbox.root().join(name);
        fs::create_dir_all(&custom).unwrap();
        dir = custom.to_str().unwrap().to_owned();
        sandbox.vars.claude_config_dir = Some(dir.clone());
        let secure = secure.map(|secure| {
            if secure.is_empty() {
                String::new()
            } else {
                let folder = sandbox.root().join(secure);
                fs::create_dir_all(&folder).unwrap();
                folder.to_str().unwrap().to_owned()
            }
        });
        sandbox.vars.securestorage_config_dir = secure;
    });
    (world, dir)
}

fn write_credentials_file(world: &World, text: &str) -> std::path::PathBuf {
    let path = paths::credentials_path(world.store.env());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn custom_config_dir_does_not_return_the_default_keychain_item() {
    let (world, _) = with_config_dir("custom-profile", None);
    let account = world.account();
    world
        .security
        .fake
        .set(CLAUDE_CODE_KEYCHAIN_SERVICE, &account, &default_creds());
    world
        .security
        .fake
        .set(MANAGED, &account, "sk-ant-api-default");

    let result = world.store.read_active_credentials();

    let seen = world.security.read_services();
    assert!(
        !seen.iter().any(|s| s == CLAUDE_CODE_KEYCHAIN_SERVICE),
        "{seen:?}"
    );
    assert!(!seen.iter().any(|s| s == MANAGED), "{seen:?}");
    assert_ne!(result.value, Some(default_creds()));
    assert_eq!(result.value.as_deref(), Some(""));
}

#[test]
fn custom_config_dir_reads_its_own_hashed_keychain_item() {
    // Claude Code writes rotations Keychain-only: a live custom profile often has no file.
    let (world, dir) = with_config_dir("custom-profile", None);
    let account = world.account();
    let hashed = keychain_service_name(&dir);
    world.security.fake.set(&hashed, &account, &custom_creds());
    world
        .security
        .fake
        .set(CLAUDE_CODE_KEYCHAIN_SERVICE, &account, &default_creds());

    let result = world.store.read_active_credentials();

    assert_eq!(result.value, Some(custom_creds()));
    assert_eq!(world.security.read_services(), [hashed]);
}

#[test]
fn custom_config_dir_falls_back_to_its_own_credentials_file() {
    let (world, _) = with_config_dir("custom-profile", None);
    write_credentials_file(&world, &custom_creds());
    world.security.fake.set(
        CLAUDE_CODE_KEYCHAIN_SERVICE,
        &world.account(),
        &default_creds(),
    );

    assert_eq!(
        world.store.read_active_credentials().value,
        Some(custom_creds())
    );
    let seen = world.security.read_services();
    assert!(!seen.iter().any(|s| s == CLAUDE_CODE_KEYCHAIN_SERVICE));
}

#[test]
fn default_profile_still_reads_the_unsuffixed_item() {
    let world = World::new();
    world.security.fake.set(
        CLAUDE_CODE_KEYCHAIN_SERVICE,
        &world.account(),
        &default_creds(),
    );

    assert_eq!(
        world.store.read_active_credentials().value,
        Some(default_creds())
    );
    assert_eq!(
        world.security.read_services(),
        [CLAUDE_CODE_KEYCHAIN_SERVICE]
    );
}

#[test]
fn config_dir_equal_to_the_default_falls_back_to_the_unsuffixed_item() {
    let mut dir = String::new();
    let world = World::with(|sandbox| {
        let default = sandbox.home().join(".claude");
        fs::create_dir_all(&default).unwrap();
        dir = default.to_str().unwrap().to_owned();
        sandbox.vars.claude_config_dir = Some(dir.clone());
    });
    world.security.fake.set(
        CLAUDE_CODE_KEYCHAIN_SERVICE,
        &world.account(),
        &default_creds(),
    );

    assert_eq!(
        world.store.read_active_credentials().value,
        Some(default_creds())
    );
    assert_eq!(
        world.security.read_services(),
        [
            keychain_service_name(&dir),
            CLAUDE_CODE_KEYCHAIN_SERVICE.to_owned()
        ]
    );
}

#[test]
fn defined_and_empty_selects_the_default_store() {
    let (world, _) = with_config_dir("custom-profile", Some(""));
    world.security.fake.set(
        CLAUDE_CODE_KEYCHAIN_SERVICE,
        &world.account(),
        &default_creds(),
    );

    assert_eq!(
        world.store.read_active_credentials().value,
        Some(default_creds())
    );
    assert_eq!(
        world.security.read_services(),
        [CLAUDE_CODE_KEYCHAIN_SERVICE]
    );
}

#[test]
fn defined_and_set_selects_that_stores_hashed_item() {
    let (world, custom) = with_config_dir("custom-profile", Some("secure-profile"));
    let secure = world
        .store
        .env()
        .vars
        .securestorage_config_dir
        .clone()
        .unwrap();
    let account = world.account();
    let fake = &world.security.fake;
    fake.set(
        &keychain_service_name(&secure),
        &account,
        &cred("secure-profile"),
    );
    fake.set(&keychain_service_name(&custom), &account, &custom_creds());
    fake.set(CLAUDE_CODE_KEYCHAIN_SERVICE, &account, &default_creds());

    assert_eq!(
        world.store.read_active_credentials().value,
        Some(cred("secure-profile"))
    );
    assert_eq!(
        world.security.read_services(),
        [keychain_service_name(&secure)]
    );
}

#[test]
fn active_write_keys_keychain_by_account_name() {
    let world = World::with(|sandbox| sandbox.vars.user = None);
    world.store.write_credentials(r#"{"x":1}"#).unwrap();
    let account = crate::keychain::keychain_account_name(None);
    let items = world.security.fake.items();
    assert!(items.contains_key(&(CLAUDE_CODE_KEYCHAIN_SERVICE.to_owned(), account)));
    // Never a "user" default that mismatches Claude Code headless.
    assert!(!items.contains_key(&(CLAUDE_CODE_KEYCHAIN_SERVICE.to_owned(), "user".to_owned())));
    assert_eq!(
        world.store.last_active_credentials_backend(),
        Some("keychain")
    );
    world.assert_no_secret_in_calls(r#"{"x":1}"#);
}

#[test]
fn active_read_prefers_keychain_then_file() {
    let world = World::new();
    let account = world.account();
    world
        .security
        .fake
        .set(CLAUDE_CODE_KEYCHAIN_SERVICE, &account, "FROM-KC");
    write_credentials_file(&world, "FROM-FILE");
    assert_eq!(world.store.read_credentials().as_deref(), Some("FROM-KC"));
    world
        .security
        .fake
        .remove(CLAUDE_CODE_KEYCHAIN_SERVICE, &account);
    assert_eq!(world.store.read_credentials().as_deref(), Some("FROM-FILE"));
}

#[test]
fn active_read_retries_transient_keychain_failure() {
    let world = World::new();
    world
        .security
        .fake
        .set(CLAUDE_CODE_KEYCHAIN_SERVICE, &world.account(), "FROM-KC");
    world.security.fake.answer_next(Answer::Exit(51));

    let result = world.store.read_active_credentials();
    assert_eq!(result.value.as_deref(), Some("FROM-KC"));
    assert!(!result.keychain_unavailable);
    assert_eq!(
        world.security.read_services().len(),
        2,
        "failed once, retried once"
    );
}

#[test]
fn active_read_keychain_unavailable_no_fallback() {
    let world = World::new();
    world.security.lock();
    assert!(!paths::credentials_path(world.store.env()).exists());

    let result = world.store.read_active_credentials();
    assert_eq!(result.value.as_deref(), Some(""));
    assert!(result.keychain_unavailable);
    // The value-only read still reads as empty.
    assert_eq!(world.store.read_credentials().as_deref(), Some(""));
}

#[test]
fn active_read_keychain_failure_covered_by_file() {
    let world = World::new();
    write_credentials_file(&world, "FROM-FILE");
    world.security.lock();
    let result = world.store.read_active_credentials();
    assert_eq!(result.value.as_deref(), Some("FROM-FILE"));
    assert!(!result.keychain_unavailable);
}

#[test]
fn active_read_absent_item_is_not_keychain_unavailable() {
    let world = World::new();
    let result = world.store.read_active_credentials();
    assert_eq!(result.value.as_deref(), Some(""));
    assert!(!result.keychain_unavailable);
    assert_eq!(
        world.security.read_services().len(),
        1,
        "nothing raised, nothing retried"
    );
}

#[test]
fn file_covered_keychain_failure_is_degraded() {
    let world = World::new();
    write_credentials_file(&world, "FROM-FILE");
    world.security.lock();
    let result = world.store.read_active_credentials();
    assert_eq!(result.value.as_deref(), Some("FROM-FILE"));
    assert!(!result.keychain_unavailable, "display contract intact");
    assert!(result.degraded, "consume ban signal");
}

/// The store's half of cswap's capture refusal: a locked Keychain with a readable plaintext
/// file reads degraded, which the switcher's `_refuse_degraded_capture` refuses.
#[test]
fn the_default_capture_path_refuses_a_degraded_read() {
    let world = World::new();
    world.store.set_keychain_usable_cache(Some(true));
    world.security.lock();
    let spent = serde_json::json!({"claudeAiOauth": {"accessToken": "sk-maybe-spent"}}).to_string();
    write_credentials_file(&world, &spent);
    let result = world.store.read_active_credentials();
    assert!(result.degraded);
    assert_eq!(result.value, Some(spent));
}

/// An unreadable file under a denied Keychain is the most unreadable state: `None`,
/// unavailable and degraded, never "no credentials".
#[test]
fn an_unreadable_file_under_a_denied_keychain_is_unavailable() {
    let world = World::new();
    let path = write_credentials_file(&world, &cred("live"));
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    world.security.lock();
    let result = world.store.read_active_credentials();
    assert_eq!(result.value, None);
    assert!(result.keychain_unavailable && result.degraded);
}

fn old_mtime(path: &std::path::Path) -> SystemTime {
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn keychain_write_refreshes_existing_file() {
    // #86: the present file is rewritten so a running session reloads; #1414: never deleted.
    let world = World::new();
    let path = write_credentials_file(&world, "OLD-CREDS");
    let before = old_mtime(&path);

    world.store.write_credentials(r#"{"fresh":1}"#).unwrap();

    assert_eq!(
        world.store.last_active_credentials_backend(),
        Some("keychain")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"fresh":1}"#);
    assert!(fs::metadata(&path).unwrap().modified().unwrap() > before);
}

#[test]
fn keychain_write_bumps_mtime_even_when_content_unchanged() {
    let world = World::new();
    let path = write_credentials_file(&world, r#"{"same":1}"#);
    let before = old_mtime(&path);
    world.store.write_credentials(r#"{"same":1}"#).unwrap();
    assert!(fs::metadata(&path).unwrap().modified().unwrap() > before);
}

#[test]
fn keychain_write_does_not_create_absent_file() {
    let world = World::new();
    let path = paths::credentials_path(world.store.env());
    world.store.write_credentials(r#"{"fresh":1}"#).unwrap();
    assert_eq!(
        world.store.last_active_credentials_backend(),
        Some("keychain")
    );
    assert!(!path.exists());
}

#[test]
fn refresh_stale_file_is_best_effort() {
    let world = World::new();
    write_credentials_file(&world, "OLD-CREDS");
    // The shadow file's rewrite fails as a full disk does.
    fsutil::FAULT.set(Some((fsutil::Step::Rename, libc::ENOSPC)));
    let written = world.store.write_credentials(r#"{"fresh":1}"#);
    fsutil::FAULT.set(None);
    written.unwrap();
    assert_eq!(
        world.store.last_active_credentials_backend(),
        Some("keychain")
    );
}

/// cswap's `test_active_write_falls_back_to_file_and_clears_stale_keychain`, with no file to
/// fall back to: the write fails, no file is written, and the Keychain item stays.
#[test]
fn active_write_failure_writes_no_file_and_keeps_the_item() {
    let world = World::new();
    let account = world.account();
    world
        .security
        .fake
        .set(CLAUDE_CODE_KEYCHAIN_SERVICE, &account, "STALE");
    world
        .security
        .set_rule(|call| (super::verb(call) == "add-generic-password").then_some(Answer::Exit(51)));

    let written = world.store.write_credentials(r#"{"fresh":1}"#);

    assert!(
        matches!(written, Err(AccountsError::CredentialWrite(_))),
        "{written:?}"
    );
    assert!(!paths::credentials_path(world.store.env()).exists());
    assert_eq!(
        world
            .security
            .fake
            .get(CLAUDE_CODE_KEYCHAIN_SERVICE, &account)
            .as_deref(),
        Some("STALE")
    );
    assert_eq!(world.store.last_active_credentials_backend(), None);
}

#[test]
fn a_managed_api_key_is_refused_not_written() {
    let world = World::new();
    let key = "sk-ant-api03-a1b2c3d4e5a1b2c3d4e5a1b2c3d4e5a1b2c3d4e5";
    let written = world.store.write_credentials(key);
    assert!(
        matches!(written, Err(AccountsError::Credential(_))),
        "{written:?}"
    );
    assert!(world.security.calls().is_empty());
}

#[test]
fn healthy_mac_reads_create_no_files() {
    let world = World::new();
    world
        .store
        .kc_write_backup("1", "a@example.com", "KC")
        .unwrap();
    assert_eq!(
        world
            .store
            .read_account_credentials("1", "a@example.com", false),
        "KC"
    );
    assert_eq!(world.store.read_credentials().as_deref(), Some(""));
    assert!(!paths::credentials_path(world.store.env()).exists());
    assert!(!world.store.credentials_dir().exists());
}

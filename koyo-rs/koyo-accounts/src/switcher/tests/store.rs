//! cswap's `tests/test_switcher.py` (0fe22a8) for the store: `TestEmailValidation`,
//! `TestFindAccountSlot`, `TestJsonOperations`, `TestDirectorySetup`,
//! `TestGetNextAccountNumber`, `TestSalvageCopyMode`, `TestSwitchTransactionRollback`, and
//! `TestSwitchSkipsBrokenSlots.test_account_is_switchable_helper`.

use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

use serde_json::json;

use super::{Harness, sample_sequence_data, seed, seed_with, tmp_files_in};
use crate::AccountsError;
use crate::fsutil::{FAULT, Step};
use crate::switcher::Switcher;
use crate::switcher::identity::validate_email;
use crate::switcher::switch::rollback::{Originals, Step as TxStep, SwitchTransaction};

#[test]
fn valid_emails() {
    for email in [
        "user@example.com",
        "user.name@example.co.uk",
        "user+tag@example.org",
        "user123@test.io",
    ] {
        assert!(validate_email(email), "Expected {email} to be valid");
    }
}

#[test]
fn invalid_emails() {
    for email in [
        "not-an-email",
        "@example.com",
        "user@",
        "user@.com",
        "",
        "user@com",
    ] {
        assert!(!validate_email(email), "Expected {email} to be invalid");
    }
}

fn slot_data() -> crate::switcher::store::Data {
    json!({"accounts": {
        "1": {"email": "user@example.com", "organizationUuid": ""},
        "2": {"email": "user@example.com", "organizationUuid": "org-123"},
        "3": {"email": "other@example.com"},
    }})
    .as_object()
    .unwrap()
    .clone()
}

#[test]
fn matches_composite_identity() {
    assert_eq!(
        Switcher::find_account_slot(&slot_data(), "user@example.com", "org-123").as_deref(),
        Some("2")
    );
}

#[test]
fn same_email_wrong_org_is_no_match() {
    assert_eq!(
        Switcher::find_account_slot(&slot_data(), "user@example.com", "org-999"),
        None
    );
}

#[test]
fn absent_email_is_no_match() {
    assert_eq!(
        Switcher::find_account_slot(&slot_data(), "nobody@example.com", ""),
        None
    );
}

#[test]
fn empty_org_matches_missing_or_empty_org_field() {
    // Slot 1 has "", slot 3 omits the field: both are personal.
    assert_eq!(
        Switcher::find_account_slot(&slot_data(), "user@example.com", "").as_deref(),
        Some("1")
    );
    assert_eq!(
        Switcher::find_account_slot(&slot_data(), "other@example.com", "").as_deref(),
        Some("3")
    );
}

#[test]
fn empty_data_is_no_match() {
    assert_eq!(
        Switcher::find_account_slot(&serde_json::Map::new(), "user@example.com", ""),
        None
    );
}

#[test]
fn write_and_read_json() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    let path = h.sandbox.store().join("test.json");
    let data = json!({"key": "value", "number": 42, "nested": {"a": 1}});
    h.switcher.write_json(&path, &data).unwrap();
    assert_eq!(
        h.switcher.read_json(&path, false).unwrap(),
        data.as_object().cloned()
    );
}

#[test]
fn read_nonexistent_json() {
    let h = Harness::new();
    let path = h.sandbox.root().join("nonexistent/path.json");
    assert_eq!(h.switcher.read_json(&path, false).unwrap(), None);
}

#[test]
fn read_invalid_json() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    let path = h.sandbox.store().join("invalid.json");
    std::fs::write(&path, "not valid json {{{").unwrap();
    assert_eq!(h.switcher.read_json(&path, false).unwrap(), None);
    // Strict, a torn file is refused rather than read as absent.
    assert!(matches!(
        h.switcher.read_json(&path, true),
        Err(AccountsError::Config(_))
    ));
}

#[test]
fn json_file_permissions() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    let path = h.sandbox.store().join("secure.json");
    h.switcher
        .write_json(&path, &json!({"secret": "data"}))
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn creates_directories() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    for dir in ["", "configs", "credentials"] {
        assert!(h.sandbox.store().join(dir).is_dir());
    }
}

#[test]
fn directory_permissions() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    for dir in ["", "configs", "credentials"] {
        let mode = std::fs::metadata(h.sandbox.store().join(dir))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "{dir}");
    }
}

#[test]
fn first_account() {
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    h.switcher.init_sequence_file().unwrap();
    assert_eq!(h.switcher.get_next_account_number().unwrap(), 1);
}

#[test]
fn with_existing_accounts() {
    let h = Harness::new();
    h.write_sequence(&sample_sequence_data());
    assert_eq!(h.switcher.get_next_account_number().unwrap(), 3);
}

#[test]
fn salvage_copy_is_private_from_creation() {
    // The aside copy can hold `primaryApiKey`: 0600 from creation, the mode call skipped.
    let h = Harness::new();
    let config_path = h.config_path();
    std::fs::write(&config_path, r#"{"primaryApiKey": "sk-test", "torn":"#).unwrap();
    FAULT.set(Some((Step::Mode, 0)));
    let salvage = h.switcher.salvage_unreadable(&config_path, &mut Vec::new());
    FAULT.set(None);
    let salvage = salvage.unwrap();
    assert_eq!(
        std::fs::read(&salvage).unwrap(),
        std::fs::read(&config_path).unwrap()
    );
    assert_eq!(
        std::fs::metadata(&salvage).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!tmp_files_in(&h.home()));
}

#[test]
fn two_salvages_in_one_second_keep_two_copies() {
    let h = Harness::new();
    let config_path = h.config_path();
    std::fs::write(&config_path, "{torn").unwrap();
    let mut warnings = Vec::new();
    let first = h
        .switcher
        .salvage_unreadable(&config_path, &mut warnings)
        .unwrap();
    let second = h
        .switcher
        .salvage_unreadable(&config_path, &mut warnings)
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].contains("could not be parsed"), "{warnings:?}");
}

fn transaction(h: &Harness, original: &str) -> (SwitchTransaction, std::path::PathBuf) {
    let dir = h.sandbox.root().join("tx");
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join(".claude.json");
    std::fs::write(&config_path, original).unwrap();
    let tx = SwitchTransaction {
        originals: Originals {
            credentials: String::new(),
            config: original.to_owned(),
            account_num: "1".into(),
            config_path: config_path.clone(),
        },
        completed_steps: vec![TxStep::ConfigWritten],
    };
    (tx, config_path)
}

#[test]
fn config_restore_is_published_by_rename() {
    let h = Harness::new();
    let original = json!({"oauthAccount": {"emailAddress": "a@example.com"}}).to_string();
    let (tx, config_path) = transaction(&h, &original);
    std::fs::write(&config_path, "{}").unwrap();
    let ino = std::fs::metadata(&config_path).unwrap().ino();
    assert!(tx.rollback(&h.switcher));
    assert_eq!(std::fs::read_to_string(&config_path).unwrap(), original);
    let meta = std::fs::metadata(&config_path).unwrap();
    assert_ne!(meta.ino(), ino);
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        std::fs::read_dir(config_path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn failed_config_restore_leaves_the_file_as_it_was() {
    let h = Harness::new();
    let original = json!({"oauthAccount": {"emailAddress": "a@example.com"}}).to_string();
    let (tx, config_path) = transaction(&h, &original);
    std::fs::write(&config_path, r#"{"half": "written"}"#).unwrap();
    FAULT.set(Some((Step::Rename, libc::ENOSPC)));
    let restored = tx.rollback(&h.switcher);
    FAULT.set(None);
    assert!(!restored);
    assert_eq!(
        std::fs::read_to_string(&config_path).unwrap(),
        r#"{"half": "written"}"#
    );
    assert_eq!(
        std::fs::read_dir(config_path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn account_is_switchable_helper() {
    let h = Harness::new();
    seed(&h, 1, "a@example.com");
    seed_with(&h, 2, "b@example.com", false, true);
    seed_with(&h, 3, "c@example.com", true, false);
    let data = h.switcher.get_sequence_data().unwrap().unwrap();
    assert!(h.switcher.account_is_switchable(&data, "1").unwrap());
    assert!(!h.switcher.account_is_switchable(&data, "2").unwrap());
    assert!(!h.switcher.account_is_switchable(&data, "3").unwrap());
    // A stale sequence reference to a missing record.
    assert!(!h.switcher.account_is_switchable(&data, "99").unwrap());
}

#[test]
fn a_torn_roster_is_refused_not_read_as_empty() {
    // `_get_sequence_data` is strict: a torn roster read as "no accounts" let `add_account`
    // reassign slot 1 over a live backup.
    let h = Harness::new();
    h.switcher.setup_directories().unwrap();
    std::fs::write(
        h.sandbox.store().join("sequence.json"),
        r#"{"accounts": {"1""#,
    )
    .unwrap();
    assert!(matches!(
        h.switcher.get_sequence_data(),
        Err(AccountsError::Config(_))
    ));
    assert!(h.switcher.get_next_account_number().is_err());
}

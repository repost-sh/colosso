//! cswap's `tests/test_macos_keychain.py`, on the fake `security`, and its real-Keychain
//! round trip (`test_macos_keychain_contract.py`) on a temporary keychain, run on its own.
//! The `item_exists` cases are left out with it.

use super::*;
use crate::testing::{Answer, FakeSecurity};

#[test]
fn get_password_returns_value_on_rc0() {
    let security = FakeSecurity::default();
    security.set("svc", "acct", "the-secret");
    assert_eq!(
        get_password(&security, "svc", "acct").unwrap().as_deref(),
        Some("the-secret")
    );
    let call = security.calls().pop().unwrap();
    assert_eq!(call.args[0], "find-generic-password");
    assert!(call.args.iter().any(|arg| arg == "-a"));
    assert!(call.args.iter().any(|arg| arg == "acct"));
    assert!(call.args.iter().any(|arg| arg == "svc"));
}

#[test]
fn get_password_returns_none_only_on_rc44() {
    let security = FakeSecurity::default();
    assert_eq!(get_password(&security, "svc", "acct").unwrap(), None);
}

#[test]
fn get_password_raises_on_other_nonzero() {
    // Locked, denied, unavailable: never masked as "not found".
    let security = FakeSecurity::default();
    security.set("svc", "acct", "the-secret");
    security.answer_next(Answer::Exit(51));
    assert!(get_password(&security, "svc", "acct").is_err());
}

#[test]
fn set_password_small_payload_uses_security_i_stdin() {
    let security = FakeSecurity::default();
    set_password(&security, "svc", "acct", "short-secret").unwrap();
    let call = security.calls().pop().unwrap();
    assert_eq!(call.args, ["-i"]);
    let stdin = call.stdin.unwrap();
    assert!(stdin.starts_with("add-generic-password -U"));
    assert!(stdin.contains(&format!("-X {}", "73686f72742d736563726574")));
    assert!(stdin.contains("-a \"acct\"") && stdin.contains("-s \"svc\""));
    assert!(!stdin.contains("short-secret"));
    assert_eq!(security.get("svc", "acct").as_deref(), Some("short-secret"));
}

/// cswap falls back to argv for a payload over the line; Koyo refuses instead, and nothing
/// is run (`docs/claude-accounts.md` 3: no token in a command's arguments).
#[test]
fn set_password_large_payload_is_refused_not_put_in_argv() {
    let security = FakeSecurity::default();
    let big = "x".repeat(SECURITY_STDIN_LINE_LIMIT);
    assert!(set_password(&security, "svc", "acct", &big).is_err());
    assert!(security.calls().is_empty());
    // The longest that fits still goes through stdin.
    let fits = "x".repeat((SECURITY_STDIN_LINE_LIMIT - 80) / 2);
    set_password(&security, "svc", "acct", &fits).unwrap();
    assert_eq!(security.calls()[0].args, ["-i"]);
}

#[test]
fn set_password_raises_on_nonzero() {
    let security = FakeSecurity::default();
    security.answer_next(Answer::Exit(45));
    assert!(set_password(&security, "svc", "acct", "secret").is_err());
}

#[test]
fn set_get_roundtrip_hex_is_decodable() {
    let security = FakeSecurity::default();
    let secret = "token-with \"quotes\" and \\ backslash and é";
    set_password(&security, "svc", "acct", secret).unwrap();
    let stdin = security.calls()[0].stdin.clone().unwrap();
    let hex = stdin.split("-X ").nth(1).unwrap().trim();
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect();
    assert_eq!(String::from_utf8(bytes).unwrap(), secret);
    assert_eq!(
        get_password(&security, "svc", "acct").unwrap().as_deref(),
        Some(secret)
    );
}

#[test]
fn delete_password_rc0_and_rc44_are_success() {
    let security = FakeSecurity::default();
    security.set("svc", "acct", "x");
    delete_password(&security, "svc", "acct").unwrap();
    delete_password(&security, "svc", "acct").unwrap();
}

#[test]
fn delete_password_raises_on_other_nonzero() {
    let security = FakeSecurity::default();
    security.answer_next(Answer::Exit(51));
    assert!(delete_password(&security, "svc", "acct").is_err());
}

#[test]
fn timeout_becomes_keychain_error() {
    let security = FakeSecurity::default();
    security.answer_all(Answer::Timeout);
    assert!(get_password(&security, "svc", "acct").is_err());
    assert!(set_password(&security, "svc", "acct", "secret").is_err());
    assert!(delete_password(&security, "svc", "acct").is_err());
}

#[test]
fn a_missing_binary_is_a_keychain_error_and_no_item() {
    struct Missing;
    impl Security for Missing {
        fn run(&self, _: &[&str], _: Option<&str>) -> Result<Output, KeychainError> {
            Err(KeychainError(
                "couldn't run /usr/bin/security: not found".into(),
            ))
        }
    }
    assert!(get_password(&Missing, "svc", "acct").is_err());
}

#[test]
fn keychain_account_name_prefers_user_env() {
    assert_eq!(keychain_account_name(Some("alice")), "alice");
}

#[test]
fn keychain_account_name_no_user_env_avoids_legacy_default() {
    let name = keychain_account_name(None);
    assert!(!name.is_empty() && name != "user");
    assert_eq!(keychain_account_name(Some("")), name);
}

/// set → get → delete through the real `/usr/bin/security`, on a keychain made for the test
/// in the build's target folder and named by every call, so neither the person's default
/// keychain nor any Claude item is read or written. macOS lists a made keychain in the
/// person's search list, which the test puts back as it was; so it runs only on its own:
/// `script/cargo test -p koyo-accounts -- --ignored real_keychain`.
#[test]
#[ignore = "makes a keychain, which macOS lists in the person's search list for the test's length"]
fn real_keychain_round_trip_on_a_temporary_keychain() {
    let dir = Path::new(env!("KOYO_TARGET_DIR")).join("accounts-keychain");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("koyo-accounts-{}.keychain-db", std::process::id()));
    let run = |args: &[&str]| Command::new(SECURITY).args(args).output().unwrap();
    let listed = String::from_utf8(run(&["list-keychains", "-d", "user"]).stdout).unwrap();
    let listed: Vec<String> = listed
        .lines()
        .map(|line| line.trim().trim_matches('"').to_owned())
        .filter(|line| !line.is_empty())
        .collect();
    let keychain = path.to_str().unwrap();
    assert!(
        run(&["create-keychain", "-p", "", keychain])
            .status
            .success()
    );
    let result = std::panic::catch_unwind(|| {
        assert!(
            run(&["unlock-keychain", "-p", "", keychain])
                .status
                .success()
        );
        let security = SystemSecurity {
            keychain: Some(path.clone()),
        };
        set_password(
            &security,
            "Koyo accounts test",
            "acct-1",
            "round-trip-token",
        )
        .unwrap();
        assert_eq!(
            get_password(&security, "Koyo accounts test", "acct-1")
                .unwrap()
                .as_deref(),
            Some("round-trip-token")
        );
        delete_password(&security, "Koyo accounts test", "acct-1").unwrap();
        assert_eq!(
            get_password(&security, "Koyo accounts test", "acct-1").unwrap(),
            None
        );
    });
    let mut restore = vec!["list-keychains", "-d", "user", "-s"];
    restore.extend(listed.iter().map(String::as_str));
    run(&restore);
    run(&["delete-keychain", keychain]);
    result.unwrap();
}

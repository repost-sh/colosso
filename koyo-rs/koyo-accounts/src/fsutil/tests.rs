//! cswap's `tests/test_fsutil.py`, the atomic writer's classes (`TestAtomicWrite`,
//! `TestAtomicWriteSymlinks`, `TestAtomicWriteTempName`); the Windows retries and the link
//! helpers aren't ported. Faults are injected at the writer's own steps (`FAULT`), as cswap
//! patches `os.fchmod`, `os.fsync` and `replace_with_retry`.

use std::os::unix::fs::{MetadataExt as _, symlink};

use super::*;
use crate::testing::Sandbox;

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn with_fault<T>(step: Step, code: i32, f: impl FnOnce() -> T) -> T {
    FAULT.set(Some((step, code)));
    let out = f();
    FAULT.set(None);
    out
}

#[test]
fn publishes_by_rename() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home();
    let target = dir.join(".claude.json");
    fs::write(&target, "old").unwrap();
    let before = fs::metadata(&target).unwrap().ino();
    atomic_write(&env, &target, b"new").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    assert_eq!(names(&dir), [".claude.json"]);
    let after = fs::metadata(&target).unwrap();
    assert_ne!(after.ino(), before, "published, not rewritten");
    assert_eq!(after.mode() & 0o777, 0o600);
}

#[test]
fn failed_publish_leaves_destination_untouched() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let target = sandbox.home().join(".claude.json");
    fs::write(&target, "old").unwrap();
    let result = with_fault(Step::Rename, libc::ENOSPC, || {
        atomic_write(&env, &target, b"new")
    });
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "old");
    assert_eq!(names(&sandbox.home()), [".claude.json"]);
}

#[test]
fn temp_is_private_from_creation() {
    // With the mode step skipped, the published mode is the one the temp was made with: a
    // write-then-chmod writer would show 0644 under the usual umask.
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let target = sandbox.home().join("secret.json");
    with_fault(Step::Mode, 0, || atomic_write(&env, &target, b"{}")).unwrap();
    assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o600);
}

#[test]
fn failing_mode_call_aborts_before_publishing() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let target = sandbox.home().join("secret.json");
    fs::write(&target, "old").unwrap();
    let result = with_fault(Step::Mode, libc::EPERM, || {
        atomic_write(&env, &target, b"new")
    });
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "old");
    assert_eq!(names(&sandbox.home()), ["secret.json"]);
}

#[test]
fn fsync_unsupported_is_tolerated_other_errors_are_not() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let target = sandbox.home().join("x.json");
    fs::write(&target, "old").unwrap();
    with_fault(Step::Fsync, libc::EINVAL, || {
        atomic_write(&env, &target, b"new")
    })
    .unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    let broken = with_fault(Step::Fsync, libc::EIO, || {
        atomic_write(&env, &target, b"newer")
    });
    assert!(broken.is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    assert_eq!(names(&sandbox.home()), ["x.json"]);
}

#[test]
fn missing_parent_is_not_created() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let target = sandbox.home().join("missing/x.json");
    assert!(atomic_write(&env, &target, b"{}").is_err());
    assert!(!sandbox.home().join("missing").exists());
}

#[test]
fn bytes_round_trip() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let target = sandbox.home().join("blob.bin");
    let payload: Vec<u8> = (0..=255u8).cycle().take(256 * 300).collect();
    atomic_write(&env, &target, &payload).unwrap();
    assert_eq!(fs::read(&target).unwrap(), payload);
    assert_eq!(names(&sandbox.home()), ["blob.bin"]);
}

#[test]
fn writes_through_a_link_and_keeps_it() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let repo = sandbox.home().join("repo");
    let live = sandbox.home().join("live");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&live).unwrap();
    let tracked = repo.join(".claude.json");
    fs::write(&tracked, "old").unwrap();
    let link = live.join(".claude.json");
    symlink(&tracked, &link).unwrap();
    atomic_write(&env, &link, b"new").unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&tracked).unwrap(), "new");
    assert_eq!(names(&repo), [".claude.json"]);
    assert_eq!(names(&live), [".claude.json"]);
}

#[test]
fn follows_a_relative_two_hop_chain() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home();
    fs::create_dir_all(dir.join("real")).unwrap();
    let last = dir.join("real/final.json");
    fs::write(&last, "old").unwrap();
    symlink("real/final.json", dir.join("hop2")).unwrap();
    symlink("hop2", dir.join("hop1")).unwrap();
    atomic_write(&env, &dir.join("hop1"), b"new").unwrap();
    assert!(
        fs::symlink_metadata(dir.join("hop1"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::symlink_metadata(dir.join("hop2"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&last).unwrap(), "new");
    assert_eq!(
        publish_target(&dir.join("hop1")).unwrap(),
        last.canonicalize().unwrap()
    );
}

#[test]
fn dangling_link_writes_where_it_points() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let gone = sandbox.home().join("gone");
    fs::create_dir_all(&gone).unwrap();
    let link = sandbox.home().join(".claude.json");
    symlink(gone.join("config.json"), &link).unwrap();
    atomic_write(&env, &link, b"{}").unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(gone.join("config.json")).unwrap(), "{}");
    assert_eq!(names(&gone), ["config.json"]);
}

#[test]
fn loop_raises_eloop_and_writes_nothing() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home();
    symlink("loop.json", dir.join("loop.json")).unwrap();
    let Err(AccountsError::Io(_, error)) = atomic_write(&env, &dir.join("loop.json"), b"{}") else {
        panic!("a loop is refused");
    };
    assert_eq!(error.raw_os_error(), Some(libc::ELOOP));
    assert_eq!(names(&dir), ["loop.json"]);
}

#[test]
fn loop_hidden_behind_dotdot_is_still_refused() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home();
    fs::write(dir.join("victim.json"), "victim").unwrap();
    symlink("loop", dir.join("loop")).unwrap();
    symlink("loop/../victim.json", dir.join("config.json")).unwrap();
    let Err(AccountsError::Io(_, error)) = atomic_write(&env, &dir.join("config.json"), b"{}")
    else {
        panic!("a loop behind .. is refused");
    };
    assert_eq!(error.raw_os_error(), Some(libc::ELOOP));
    assert_eq!(
        fs::read_to_string(dir.join("victim.json")).unwrap(),
        "victim"
    );
    assert_eq!(names(&dir), ["config.json", "loop", "victim.json"]);
}

#[test]
fn publish_target_of_a_plain_path_is_the_path() {
    let sandbox = Sandbox::new();
    let plain = sandbox.home().join("plain.json");
    assert_eq!(publish_target(&plain).unwrap(), plain);
    fs::write(&plain, "x").unwrap();
    assert_eq!(publish_target(&plain).unwrap(), plain);
}

#[test]
fn a_destination_name_near_name_max_still_publishes() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let name = format!("{}.cswap", "x".repeat(245));
    let target = sandbox.home().join(&name);
    fs::write(&target, "old").unwrap();
    atomic_write(&env, &target, b"new").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    assert_eq!(names(&sandbox.home()), [name]);
}

#[test]
fn every_write_asks_the_guard_first() {
    // Mutation check for the wiring (pitfall 48): with a sandbox's env, a write just outside
    // its folder is refused and nothing lands there.
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let outside = sandbox.root().with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    let target = outside.join("x.json");
    assert!(matches!(
        atomic_write(&env, &target, b"{}"),
        Err(AccountsError::Guarded(_))
    ));
    assert!(create_dir_all(&env, &outside.join("d")).is_err());
    assert!(mkdir(&env, &outside.join("lock")).is_err());
    assert!(open_for_lock(&env, &outside.join(".lock")).is_err());
    assert!(names(&outside).is_empty());
    fs::remove_dir_all(&outside).unwrap();
}

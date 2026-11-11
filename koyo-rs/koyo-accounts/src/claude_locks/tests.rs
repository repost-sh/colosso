//! cswap's `tests/test_claude_locks.py`: `TestProperLockfile`, `TestLockPaths` and
//! `TestCcRefreshLockProtocol`, on real directories in a sandbox. The cases that patch the
//! clock and `sleep` to cross a deadline mid-arm are covered by `nap`'s clamp, which every arm
//! goes through.

use std::fs;
use std::sync::{Arc, Barrier};

use super::*;
use crate::testing::Sandbox;

/// Ages `path` by `by`, as cswap's tests `os.utime` a lock into the past.
fn age_by(path: &Path, by: Duration) {
    let then = SystemTime::now() - by;
    fs::File::open(path).unwrap().set_modified(then).unwrap();
}

const SHORT: Option<Duration> = Some(Duration::from_millis(300));

#[test]
fn acquire_creates_and_release_removes() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    let lock = proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS).unwrap();
    assert!(dir.is_dir());
    drop(lock);
    assert!(!dir.exists());
}

#[test]
fn reacquire_after_release() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    drop(proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS).unwrap());
    drop(proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS).unwrap());
    assert!(!dir.exists());
}

#[test]
fn contention_times_out() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    fs::create_dir(&dir).unwrap();
    let start = Instant::now();
    let held = proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS);
    assert!(matches!(held, Err(AccountsError::ClaudeCodeLockTimeout(_))));
    assert!(start.elapsed() < Duration::from_secs(2));
    // Someone else's lock is left as it was.
    assert!(dir.is_dir());
}

#[test]
fn stale_lock_is_taken_over() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    fs::create_dir(&dir).unwrap();
    age_by(&dir, Duration::from_secs(30));
    let lock = proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS).unwrap();
    assert!(dir.is_dir());
    drop(lock);
    assert!(!dir.exists());
}

#[test]
fn release_tolerates_stolen_lock() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    let lock = proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS).unwrap();
    fs::remove_dir(&dir).unwrap();
    drop(lock);
}

#[test]
fn toucher_keeps_mtime_fresh() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    let lock = proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS).unwrap();
    age_by(&dir, Duration::from_secs(8));
    fsutil::nap(TOUCH_INTERVAL + Duration::from_millis(600));
    assert!(
        age(&dir, true).unwrap() < Duration::from_secs(2),
        "touched while held"
    );
    drop(lock);
}

#[test]
fn creates_missing_parent() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("deep/er/x.lock");
    drop(proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS).unwrap());
    assert!(sandbox.home().join("deep/er").is_dir());
}

#[test]
fn two_waiters_cannot_both_take_over_one_stale_lock() {
    let sandbox = Sandbox::new();
    let dir = sandbox.home().join("x.lock");
    fs::create_dir(&dir).unwrap();
    age_by(&dir, Duration::from_secs(30));
    let barrier = Arc::new(Barrier::new(2));
    let holders = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let most = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (env, dir) = (sandbox.env(), dir.clone());
            let (barrier, holders, most) = (barrier.clone(), holders.clone(), most.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let lock =
                    proper_lockfile(&env, &dir, Some(Duration::from_secs(3)), CONFIG_STALENESS)
                        .unwrap();
                let now = holders.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                most.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                fsutil::nap(Duration::from_millis(200));
                holders.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                drop(lock);
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(
        most.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "never two holders"
    );
}

#[test]
fn takeover_leaves_no_takeover_dir() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    fs::create_dir(&dir).unwrap();
    age_by(&dir, Duration::from_secs(30));
    drop(proper_lockfile(&env, &dir, SHORT, CONFIG_STALENESS).unwrap());
    assert!(!sandbox.home().join("x.lock.takeover").exists());
}

#[test]
fn dead_takeover_dir_does_not_block_takeover() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    let takeover = sandbox.home().join("x.lock.takeover");
    fs::create_dir(&dir).unwrap();
    fs::create_dir(&takeover).unwrap();
    age_by(&dir, Duration::from_secs(30));
    age_by(&takeover, TAKEOVER_STALENESS + Duration::from_secs(5));
    drop(proper_lockfile(&env, &dir, Some(Duration::from_secs(2)), CONFIG_STALENESS).unwrap());
    assert!(!takeover.exists());
}

#[test]
fn a_fresh_takeover_dir_is_respected() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = sandbox.home().join("x.lock");
    fs::create_dir(&dir).unwrap();
    fs::create_dir(sandbox.home().join("x.lock.takeover")).unwrap();
    age_by(&dir, Duration::from_secs(30));
    // Another waiter is mid-takeover: this one doesn't remove the lock under it.
    assert!(!remove_if_still_stale(&env, &dir, CONFIG_STALENESS));
    assert!(dir.is_dir());
}

#[test]
fn default_paths() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    assert_eq!(
        credentials_lock_dir(&env),
        sandbox.home().join(".claude.lock")
    );
    assert_eq!(
        config_lock_dir(&env),
        sandbox.home().join(".claude.json.lock")
    );
    assert_eq!(
        oauth_refresh_lock_dir(&env),
        sandbox.home().join(".claude/.oauth_refresh.lock")
    );
}

#[test]
fn claude_config_dir_is_honored() {
    let mut sandbox = Sandbox::new();
    let custom = sandbox.root().join("custom");
    sandbox.vars.claude_config_dir = Some(custom.to_str().unwrap().to_owned());
    let env = sandbox.env();
    assert_eq!(
        credentials_lock_dir(&env),
        sandbox.root().join("custom.lock")
    );
    assert_eq!(config_lock_dir(&env), custom.join(".claude.json.lock"));
    assert_eq!(
        oauth_refresh_lock_dir(&env),
        custom.join(".oauth_refresh.lock")
    );
}

#[test]
fn credentials_lock_takes_both_locks_and_the_config_lock_its_own() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let both = claude_credentials_lock(&env, SHORT).unwrap();
    assert!(oauth_refresh_lock_dir(&env).is_dir());
    assert!(credentials_lock_dir(&env).is_dir());
    let config = claude_config_lock(&env, SHORT).unwrap();
    assert!(config_lock_dir(&env).is_dir());
    drop(config);
    drop(both);
    assert!(!oauth_refresh_lock_dir(&env).exists());
    assert!(!credentials_lock_dir(&env).exists());
    assert!(!config_lock_dir(&env).exists());
}

#[test]
fn primary_contention_never_touches_legacy() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    fs::create_dir_all(oauth_refresh_lock_dir(&env)).unwrap();
    assert!(claude_credentials_lock(&env, SHORT).is_err());
    assert!(!credentials_lock_dir(&env).exists());
}

#[test]
fn legacy_contention_releases_primary() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    fs::create_dir_all(credentials_lock_dir(&env)).unwrap();
    assert!(claude_credentials_lock(&env, SHORT).is_err());
    assert!(
        !oauth_refresh_lock_dir(&env).exists(),
        "the primary was let go"
    );
    assert!(
        credentials_lock_dir(&env).is_dir(),
        "the holder's lock stays"
    );
}

#[test]
fn credentials_staleness_is_60s_not_10s_for_both_locks() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    for held in [oauth_refresh_lock_dir(&env), credentials_lock_dir(&env)] {
        fs::create_dir_all(&held).unwrap();
        age_by(&held, Duration::from_secs(30));
        assert!(
            claude_credentials_lock(&env, SHORT).is_err(),
            "{}",
            held.display()
        );
        assert!(held.is_dir());
        fs::remove_dir(&held).unwrap();
    }
}

#[test]
fn credentials_lock_stale_past_60s_is_taken_over() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let legacy = credentials_lock_dir(&env);
    fs::create_dir_all(&legacy).unwrap();
    age_by(&legacy, Duration::from_secs(61));
    drop(claude_credentials_lock(&env, SHORT).unwrap());
    assert!(!legacy.exists());
}

#[test]
fn config_lock_staleness_stays_10s() {
    let sandbox = Sandbox::new();
    let env = sandbox.env();
    let dir = config_lock_dir(&env);
    fs::create_dir_all(&dir).unwrap();
    age_by(&dir, Duration::from_secs(11));
    drop(claude_config_lock(&env, SHORT).unwrap());
}

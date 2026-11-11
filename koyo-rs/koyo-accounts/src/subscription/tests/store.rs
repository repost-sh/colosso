//! From claude-swap's `tests/test_subscription.py` (0fe22a8):
//! `TestPlanStore`: `subscription_plans.json` under the store's cache folder.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::super::*;
use super::{active_max, lapsed, team};
use crate::Clock;
use crate::testing::Sandbox;

fn updates(key: &str, update: Value) -> Map<String, Value> {
    Map::from_iter([(key.to_owned(), update)])
}

fn plan_record(plan: Map<String, Value>, fetched_at: f64) -> Value {
    Value::Object(Map::from_iter([
        ("plan".to_owned(), Value::Object(plan)),
        ("fetchedAt".to_owned(), json!(fetched_at)),
    ]))
}

fn failure(retry_at: f64, attempted_at: f64) -> Value {
    json!({"retryAt": retry_at, "attemptedAt": attempted_at})
}

/// A sandbox whose clock reads `now`.
fn at(now: f64) -> Sandbox {
    let sandbox = Sandbox::new();
    sandbox.clock.set(now);
    sandbox
}

#[test]
fn round_trip() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store.save(&updates("k", plan_record(team(), 1.))).unwrap();
    assert_eq!(
        Value::Object(store.load()),
        json!({"k": {"plan": team(), "fetchedAt": 1.0}})
    );
}

#[test]
fn save_merges_with_another_writer() {
    let sandbox = at(1000.);
    let (first, second) = (
        PlanStore::new(&sandbox.env()),
        PlanStore::new(&sandbox.env()),
    );
    first.save(&updates("a", plan_record(team(), 1.))).unwrap();
    second
        .save(&updates("b", plan_record(lapsed(), 2.)))
        .unwrap();
    let keys: Vec<String> = first.load().keys().cloned().collect();
    assert_eq!(keys, ["a", "b"]);
}

#[test]
fn an_older_answer_cannot_replace_a_newer_plan() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store
        .save(&updates("k", plan_record(lapsed(), 200.)))
        .unwrap();
    store
        .save(&updates("k", plan_record(active_max(), 100.)))
        .unwrap();
    assert_eq!(store.load()["k"]["plan"], Value::Object(lapsed()));
}

#[test]
fn an_older_failure_cannot_back_off_a_newer_reading() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store
        .save(&updates("k", plan_record(team(), 200.)))
        .unwrap();
    store.save(&updates("k", failure(900., 100.))).unwrap();
    assert!(store.load()["k"].get("retryAt").is_none());
}

#[test]
fn a_failure_keeps_the_plan_it_failed_to_replace() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store
        .save(&updates("k", plan_record(team(), 100.)))
        .unwrap();
    store.save(&updates("k", failure(900., 200.))).unwrap();
    let record = &store.load()["k"];
    assert_eq!(record["plan"], Value::Object(team()));
    assert_eq!(record["fetchedAt"], 100.0);
    assert_eq!(record["retryAt"], 900.0);
}

#[test]
fn an_older_failure_cannot_shorten_a_newer_retry() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store
        .save(&updates("k", failure(200. + 3600., 200.)))
        .unwrap();
    store
        .save(&updates("k", failure(100. + 900., 100.)))
        .unwrap();
    assert_eq!(store.load()["k"], failure(3800., 200.));
}

#[test]
fn a_future_stamped_plan_yields_to_a_fresh_reading() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store
        .save(&updates("k", plan_record(active_max(), 5000.)))
        .unwrap();
    store
        .save(&updates("k", plan_record(lapsed(), 1000.)))
        .unwrap();
    assert_eq!(store.load()["k"], plan_record(lapsed(), 1000.));
}

#[test]
fn a_future_stamped_plan_still_takes_a_failures_backoff() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store
        .save(&updates("k", plan_record(active_max(), 5000.)))
        .unwrap();
    store.save(&updates("k", failure(1900., 1000.))).unwrap();
    let record = store.load()["k"].clone();
    assert_eq!(record["plan"], Value::Object(active_max()));
    assert_eq!(record["retryAt"], 1900.0);
    assert!(!is_due(Some(&record), 1000., PLAN_TTL_S));
}

#[test]
fn a_future_stamped_failure_does_not_hold_a_fresh_reading() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store.save(&updates("k", failure(9000., 5000.))).unwrap();
    store
        .save(&updates("k", plan_record(team(), 1000.)))
        .unwrap();
    assert_eq!(store.load()["k"], plan_record(team(), 1000.));
}

/// The clock of cswap's `test_a_record_landing_during_the_lock_wait_is_not_skew`: read under
/// the lock, it lands the other writer's newer record first (as if it had landed while this
/// save waited) and reads 1010; read before the lock, it reads 1000 and lands nothing.
struct LockWaitClock {
    env: Env,
    lock: PathBuf,
    landed: AtomicBool,
}

impl Clock for LockWaitClock {
    fn now(&self) -> f64 {
        let held = FileLock::acquire(&self.env, &self.lock, Duration::ZERO)
            .unwrap()
            .is_none();
        if !held {
            return 1000.;
        }
        if !self.landed.swap(true, Ordering::SeqCst) {
            let other = json!({"k": {"plan": lapsed(), "fetchedAt": 1005.0}});
            fsutil::atomic_write(
                &self.env,
                &PlanStore::new(&self.env).path,
                other.to_string().as_bytes(),
            )
            .unwrap();
        }
        1010.
    }
}

#[test]
fn a_record_landing_during_the_lock_wait_is_not_skew() {
    let sandbox = Sandbox::new();
    let plain = sandbox.env();
    let mut lock = PlanStore::new(&plain).path.into_os_string();
    lock.push(".lock");
    let clock = LockWaitClock {
        env: plain.clone(),
        lock: lock.into(),
        landed: false.into(),
    };
    let env = Env {
        clock: Arc::new(clock),
        ..plain
    };
    let store = PlanStore::new(&env);
    store
        .save(&updates("k", plan_record(active_max(), 990.)))
        .unwrap();
    assert_eq!(store.load()["k"], plan_record(lapsed(), 1005.));
}

#[test]
fn writers_cannot_interleave_a_read_merge_write() {
    let sandbox = at(1000.);
    let env = sandbox.env();
    std::thread::scope(|scope| {
        for i in 0..8 {
            let env = &env;
            scope.spawn(move || {
                PlanStore::new(env)
                    .save(&updates(&format!("k{i}"), plan_record(team(), 1.)))
                    .unwrap();
            });
        }
    });
    assert_eq!(PlanStore::new(&env).load().len(), 8);
}

#[test]
fn corrupt_file_reads_as_empty() {
    let sandbox = Sandbox::new();
    let store = PlanStore::new(&sandbox.env());
    std::fs::create_dir_all(store.path.parent().unwrap()).unwrap();
    std::fs::write(&store.path, "{ not json").unwrap();
    assert!(store.load().is_empty());
}

#[test]
fn missing_file_reads_as_empty() {
    assert!(PlanStore::new(&Sandbox::new().env()).load().is_empty());
}

/// cswap's `test_lands_in_the_switchers_own_backup_dir`, for the store: the file is under the
/// store's cache folder.
#[test]
fn lands_in_the_stores_own_cache_dir() {
    let sandbox = at(1000.);
    let store = PlanStore::new(&sandbox.env());
    store.save(&updates("k", plan_record(team(), 1.))).unwrap();
    let path = paths::store::cache_dir(&sandbox.env()).join(PLAN_FILENAME);
    assert_eq!(store.path, path);
    assert!(path.is_file());
}

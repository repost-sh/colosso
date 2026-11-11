//! Part of the port of claude-swap's `usage_store.py` (0fe22a8): `UsageStore`, the
//! `cache/usage.json` table. Every write is read-modify-write under
//! `cache/.usage.lock` (`locking::FileLock`, cswap's default 10 s) and published by
//! `fsutil::atomic_write_json` (0600, its folder 0700); reads take no lock. The clock is
//! `env.clock`.
//!
//! Every method takes the caller's identities (slot → `(email, organizationUuid)`) and touches
//! only those slots: a row whose stored identity differs is invisible to reads and replaced on
//! write, so a reused slot never serves the previous account's usage. A slot missing from the
//! identities is skipped (cswap raises `KeyError`). The rules over one row are in `rows.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use super::backoff::{TRUST_MAX_AGE_S, earliest_reset, rejection_deadline};
use super::rows::{Row, apply_outcome, entry_of, opt, row_eligible};
use super::{CLAIM_TTL_S, FetchRecord, Identities, Identity, PollPlan, SCHEMA_VERSION, UsageEntry};
use crate::locking::{DEFAULT_TIMEOUT, FileLock};
use crate::oauth::value::{num_or_none, py_number};
use crate::poll_policy::random_u64;
use crate::{Env, Result, fsutil, paths};

/// The `cache/usage.json` table of Koyo's store.
pub struct UsageStore {
    env: Env,
    pub path: PathBuf,
    lock_path: PathBuf,
}

/// A fencing id, cswap's `uuid.uuid4().hex`: 32 random hex digits.
fn claim_id() -> String {
    format!("{:016x}{:016x}", random_u64(), random_u64())
}

fn matches(row: Option<&Value>, identity: &Identity) -> bool {
    row.and_then(Value::as_object)
        .is_some_and(|row| matches_row(row, identity))
}

/// cswap's `_matches`: the email, and `organizationUuid` (missing reads as "").
fn matches_row(row: &Row, identity: &Identity) -> bool {
    let org_matches = match row.get("organizationUuid") {
        None => identity.1.is_empty(),
        Some(org) => org.as_str() == Some(identity.1.as_str()),
    };
    row.get("email").and_then(Value::as_str) == Some(identity.0.as_str()) && org_matches
}

/// The row for `num`, taken out to be changed and put back: a fresh one when missing or
/// another account's.
fn take_row(rows: &mut Row, num: &str, identity: &Identity) -> Row {
    match rows.remove(num) {
        Some(Value::Object(row)) if matches_row(&row, identity) => row,
        _ => fresh_row(identity),
    }
}

fn fresh_row(identity: &Identity) -> Row {
    let mut row = Map::new();
    row.insert("email".to_owned(), Value::from(identity.0.clone()));
    row.insert(
        "organizationUuid".to_owned(),
        Value::from(identity.1.clone()),
    );
    row
}

impl UsageStore {
    /// The table of `env`'s store, `paths::store::cache_dir(env)/usage.json`.
    pub fn new(env: &Env) -> UsageStore {
        let cache = paths::store::cache_dir(env);
        UsageStore {
            env: env.clone(),
            path: cache.join("usage.json"),
            lock_path: cache.join(".usage.lock"),
        }
    }

    fn lock(&self) -> Result<FileLock> {
        FileLock::hold(&self.env, &self.lock_path, DEFAULT_TIMEOUT)
    }

    /// The stored rows; a missing, unreadable, legacy or future file reads as empty.
    fn read_rows(&self) -> Row {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Map::new();
        };
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(mut raw))
                if raw.get("schemaVersion").and_then(Value::as_i64) == Some(SCHEMA_VERSION) =>
            {
                match raw.remove("accounts") {
                    Some(Value::Object(rows)) => rows,
                    _ => Map::new(),
                }
            }
            _ => Map::new(),
        }
    }

    fn write_rows(&self, rows: &Row) -> Result<()> {
        let data = json!({"schemaVersion": SCHEMA_VERSION, "accounts": rows});
        fsutil::atomic_write_json(&self.env, &self.path, &data)
    }

    /// The identity-guarded entries of the given slots (an empty entry for a missing row or
    /// another account's). `models` are the configured per-model windows, so the 429 trust also
    /// honours their resets, as the scheduler does.
    pub fn entries(
        &self,
        identities: &Identities,
        models: &[&str],
    ) -> BTreeMap<String, UsageEntry> {
        let now = self.env.now();
        let rows = self.read_rows();
        let mut out = BTreeMap::new();
        for (num, identity) in identities {
            let row = rows.get(num.as_str());
            let Some(row) = row
                .filter(|row| matches(Some(row), identity))
                .and_then(Value::as_object)
            else {
                out.insert(num.clone(), UsageEntry::default());
                continue;
            };
            out.insert(num.clone(), entry_of(row, now, models));
        }
        out
    }

    /// Read-modify-write of the rows for `nums` under the lock.
    fn mutate(
        &self,
        identities: &Identities,
        nums: &[&str],
        mut mutator: impl FnMut(&str, &mut Row),
    ) -> Result<()> {
        let _lock = self.lock()?;
        let mut rows = self.read_rows();
        for &num in nums {
            let Some(identity) = identities.get(num) else {
                continue;
            };
            let mut row = take_row(&mut rows, num, identity);
            mutator(num, &mut row);
            rows.insert(num.to_owned(), Value::Object(row));
        }
        self.write_rows(&rows)
    }

    /// Leases the slots about to be fetched, returning their fencing ids.
    pub fn claim(
        &self,
        nums: &[&str],
        identities: &Identities,
    ) -> Result<BTreeMap<String, String>> {
        if nums.is_empty() {
            return Ok(BTreeMap::new());
        }
        let now = self.env.now();
        let claims: BTreeMap<String, String> = nums
            .iter()
            .map(|num| ((*num).to_owned(), claim_id()))
            .collect();
        self.mutate(identities, nums, |num, row| {
            row.insert("lastAttemptAt".to_owned(), Value::from(now));
            row.insert("claimId".to_owned(), Value::from(claims[num].clone()));
            row.insert("claimUntil".to_owned(), Value::from(now + CLAIM_TTL_S));
        })?;
        Ok(claims
            .into_iter()
            .filter(|(num, _)| identities.contains_key(num))
            .collect())
    }

    /// Wins the right to fetch: re-checks eligibility and stamps a lease in one locked pass,
    /// returning slot → fencing id. Deciding on a lock-free read and claiming apart lets two
    /// collectors both fetch; the locked re-check closes that.
    ///
    /// Eligible: not quarantined, not in backoff, not held, not claimed, and then by mode
    /// ([`row_eligible`]): `respect_plans` (on-demand callers) needs the entry stale and
    /// poll-due; otherwise (the auto engine) poll-due or stale. `repair_overslept` also lets
    /// an impossible reset-parked plan be due, re-checked under this lock.
    pub fn reserve(
        &self,
        nums: &[&str],
        identities: &Identities,
        respect_plans: bool,
        repair_overslept: bool,
    ) -> Result<BTreeMap<String, String>> {
        if nums.is_empty() {
            return Ok(BTreeMap::new());
        }
        let now = self.env.now();
        let mut won = BTreeMap::new();
        let _lock = self.lock()?;
        let mut rows = self.read_rows();
        for &num in nums {
            let Some(identity) = identities.get(num) else {
                continue;
            };
            let eligible = !matches(rows.get(num), identity)
                || rows
                    .get(num)
                    .and_then(Value::as_object)
                    .is_some_and(|row| row_eligible(row, now, respect_plans, repair_overslept));
            if !eligible {
                continue;
            }
            let mut row = take_row(&mut rows, num, identity);
            let id = claim_id();
            row.insert("lastAttemptAt".to_owned(), Value::from(now));
            row.insert("claimId".to_owned(), Value::from(id.clone()));
            row.insert("claimUntil".to_owned(), Value::from(now + CLAIM_TTL_S));
            rows.insert(num.to_owned(), Value::Object(row));
            won.insert(num.to_owned(), id);
        }
        if !won.is_empty() {
            self.write_rows(&rows)?;
        }
        Ok(won)
    }

    /// Merges outcomes, fenced by the leases that produced them; returns the slots accepted.
    ///
    /// A late writer whose lease or identity was replaced is ignored without touching the newer
    /// row. Success and failure are exclusive writers: success resets the failure fields,
    /// failure never touches `lastGood`/`fetchedAt`. A success's plan commits with its
    /// measurement. A sentinel clears only the claim, saving the refused-credential stamp it may
    /// carry. An unfenced caller (`claims` `None`) defers to a live lease, never to an expired one.
    pub fn record(
        &self,
        outcomes: &BTreeMap<String, FetchRecord>,
        identities: &Identities,
        claims: Option<&BTreeMap<String, String>>,
        plans: Option<&BTreeMap<String, PollPlan>>,
    ) -> Result<BTreeSet<String>> {
        let mut accepted = BTreeSet::new();
        if outcomes.is_empty() {
            return Ok(accepted);
        }
        let now = self.env.now();
        let _lock = self.lock()?;
        let mut rows = self.read_rows();
        for (num, outcome) in outcomes {
            let Some(identity) = identities.get(num) else {
                continue;
            };
            let current = rows.get(num.as_str());
            let stored_claim = current
                .and_then(|row| row.get("claimId"))
                .filter(|id| !id.is_null());
            if let Some(claims) = claims {
                let fenced = claims.get(num).is_some_and(|expected| {
                    matches(current, identity)
                        && stored_claim.and_then(Value::as_str) == Some(expected.as_str())
                });
                if !fenced {
                    continue;
                }
            } else if current.is_some_and(Value::is_object)
                && stored_claim.is_some()
                // Only a live lease defers an unfenced writer; a crashed claimer's leftover
                // ticket must age out, not block forever.
                && now < num_or_none(current.and_then(|row| row.get("claimUntil"))).unwrap_or(0.)
            {
                continue;
            }
            let mut row = take_row(&mut rows, num, identity);
            apply_outcome(
                &mut row,
                outcome,
                plans.and_then(|plans| plans.get(num)),
                now,
            );
            rows.insert(num.clone(), Value::Object(row));
            accepted.insert(num.clone());
        }
        if !accepted.is_empty() {
            self.write_rows(&rows)?;
        }
        Ok(accepted)
    }

    /// Merges readings another machine took for the same accounts: slot → `(usage, age_s)`, the
    /// reading as `oauth::build_usage_result` makes it and its age when handed over (an age, so
    /// the clocks needn't agree). A reading replaces `lastGood` only when newer than the stored
    /// one. Fetch state is left alone, but a confirmed refusal's deadline extends `backoffUntil`
    /// (never shortening it), as the refusal binds every machine.
    ///
    /// `hold_s` stamps `heldUntil`: no collector fetches the slot before then and its reading
    /// stays decision-trusted. The hold never runs past the kept reading's earliest reset (every
    /// scoped window counts) nor [`TRUST_MAX_AGE_S`] past its measurement; the latest hold
    /// replaces an earlier one, `0` lifts it, `None` leaves it. Returns the slots whose
    /// `lastGood` was replaced.
    pub fn adopt(
        &self,
        readings: &BTreeMap<String, (Value, f64)>,
        identities: &Identities,
        hold_s: Option<f64>,
    ) -> Result<BTreeSet<String>> {
        let mut adopted = BTreeSet::new();
        if readings.is_empty() {
            return Ok(adopted);
        }
        let now = self.env.now();
        let nums: Vec<&str> = readings.keys().map(String::as_str).collect();
        self.mutate(identities, &nums, |num, row| {
            let (usage, age_s) = &readings[num];
            let fetched_at = now - age_s.max(0.);
            let mut stored = num_or_none(row.get("fetchedAt"));
            if stored.is_none_or(|stored| fetched_at > stored) {
                row.insert("lastGood".to_owned(), usage.clone());
                row.insert("fetchedAt".to_owned(), Value::from(fetched_at));
                stored = Some(fetched_at);
                adopted.insert(num.to_owned());
                let current = num_or_none(row.get("backoffUntil"));
                if let Some(deadline) = rejection_deadline(Some(usage), now)
                    && current.is_none_or(|current| deadline > current)
                {
                    row.insert("backoffUntil".to_owned(), Value::from(deadline));
                }
            }
            let Some(hold_s) = hold_s else {
                return;
            };
            let mut held_until = (now + hold_s).min(stored.unwrap_or(now) + TRUST_MAX_AGE_S);
            // Every scoped window, not only the models this machine watches: the store doesn't
            // know them, and a hold lifted early costs one fetch at most.
            if let Some(reset) = earliest_reset(row.get("lastGood"), &["all"]) {
                held_until = held_until.min(reset);
            }
            if held_until > now {
                row.insert("heldUntil".to_owned(), Value::from(held_until));
            } else {
                row.remove("heldUntil");
            }
        })?;
        Ok(adopted)
    }

    /// Stores the scheduler's per-slot `(nextPollAt, pollIntervalS)`.
    pub fn set_poll_plan(
        &self,
        plans: &BTreeMap<String, PollPlan>,
        identities: &Identities,
    ) -> Result<()> {
        if plans.is_empty() {
            return Ok(());
        }
        let nums: Vec<&str> = plans.keys().map(String::as_str).collect();
        self.mutate(identities, &nums, |num, row| {
            let (next_poll_at, interval) = plans[num];
            row.insert("nextPollAt".to_owned(), opt(next_poll_at));
            row.insert("pollIntervalS".to_owned(), opt(interval));
        })
    }

    /// Pulls one slot's poll plan earlier, never later: to `max(now, fetchedAt + floor)` with
    /// the interval `floor`, unless its plan already comes due by then. A slot never measured
    /// (no `fetchedAt`) or whose row is another account's is left as it is, so nothing blocks
    /// its first fetch. Read and written under the store's lock.
    ///
    /// # Errors
    /// The lock's, or the write's.
    pub fn pull_plan_earlier(&self, num: &str, identity: &Identity, floor: f64) -> Result<()> {
        let _lock = self.lock()?;
        let mut rows = self.read_rows();
        let Some(row) = rows
            .get(num)
            .and_then(Value::as_object)
            .filter(|row| matches_row(row, identity))
        else {
            return Ok(());
        };
        let Some(fetched_at) = row.get("fetchedAt").and_then(py_number) else {
            return Ok(());
        };
        let next_poll = self.env.now().max(fetched_at + floor);
        if num_or_none(row.get("nextPollAt")).is_some_and(|next| next <= next_poll) {
            return Ok(());
        }
        let mut row = take_row(&mut rows, num, identity);
        row.insert("nextPollAt".to_owned(), Value::from(next_poll));
        row.insert("pollIntervalS".to_owned(), Value::from(floor));
        rows.insert(num.to_owned(), Value::Object(row));
        self.write_rows(&rows)
    }

    /// Lifts the dead-token quarantine of slots whose credential a re-login or add rewrote: the
    /// strikes and the failure state riding with them no longer hold, and the next pass must be
    /// free to prove the new token good.
    pub fn clear_dead_token(&self, nums: &[&str], identities: &Identities) -> Result<()> {
        if nums.is_empty() {
            return Ok(());
        }
        self.mutate(identities, nums, |_, row| {
            row.insert("claimId".to_owned(), Value::Null);
            row.insert("claimUntil".to_owned(), Value::from(0.));
            row.insert("authDeadStrikes".to_owned(), Value::from(0));
            row.insert("struckFingerprint".to_owned(), Value::Null);
            row.insert("consecutiveFailures".to_owned(), Value::from(0));
            row.insert("lastError".to_owned(), Value::Null);
            row.insert("backoffUntil".to_owned(), Value::Null);
        })
    }
}

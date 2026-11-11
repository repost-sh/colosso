//! Port of claude-swap's `switcher.py` (0fe22a8), one collection pass,
//! `_collect_usage_entries`, with the poll plans a success commits (`_plans_after_fetch`).
//!
//! A pass nominates the accounts it may fetch, wins them in one locked step
//! (`UsageStore::reserve`: backoff, holds, leases, poll plans), fetches with no lock held, and
//! records the outcomes fenced by the leases, a success's plan with it. A failure only moves
//! the row's error and backoff, so the last good reading keeps being served. Sentinels are
//! laid over the entries the pass returns: the static ones, and `token expired` for an expired
//! active credential the pass doesn't reach, so the engine holds rather than fails over across
//! the gap. Candidates the lock-free read already rules out (`UsageEntry::eligible`, the rule
//! `reserve` re-checks under the lock) are dropped first, so a pass with nothing due takes no
//! lock.
//!
//! Left out: the dead-token quarantine (`_entry_token_dead`, `_slot_token_dead`, the heal of a
//! stale strike and the re-check after the fetch). Only `invalid_grant` and `no_refresh_token`
//! strike, which a usage fetch returns only through the refresh inside it; Koyo's reads
//! never refresh (`classify_usage_error` returns neither), so no pass records a strike, and the
//! auto-switch engine quarantines a dead grant in its own state.

use std::collections::{BTreeMap, BTreeSet};

use crate::Result;
use crate::oauth::{USAGE_TOKEN_EXPIRED, extract_oauth_data, is_oauth_token_expired};
use crate::poll_policy::{PlanInput, plan_after_fetch, random_unit};
use crate::switcher::Switcher;
use crate::switcher::info::AccountInfo;
use crate::switcher::usage::PollInputs;
use crate::usage_store::{FetchRecord, Identities, PollPlan, UsageEntry, with_sentinel};

/// Which accounts a pass may fetch: cswap's `fetch` and `scheduled` arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch<'a> {
    /// cswap's `fetch=None`, for on-demand callers: every stale account, respecting the stored
    /// poll plans (and repairing a plan parked past an impossible reset).
    Any,
    /// An explicit set (the engine's scheduler; empty fetches nothing), whose members may beat
    /// the serve TTL when their plan says so. `scheduled` keeps a valid future plan while
    /// letting a due one through.
    Only {
        only: &'a BTreeSet<String>,
        scheduled: bool,
    },
}

impl Fetch<'_> {
    /// Whether the pass may fetch slot `num`.
    pub fn allows(&self, num: &str) -> bool {
        match self {
            Fetch::Any => true,
            Fetch::Only { only, .. } => only.contains(num),
        }
    }

    /// An empty set: no request at all, for plans either.
    pub fn fetches_nothing(&self) -> bool {
        matches!(self, Fetch::Only { only, .. } if only.is_empty())
    }

    /// [`crate::usage_store::UsageStore::reserve`]'s `respect_plans`: an on-demand pass needs
    /// a row stale and its plan due.
    pub(crate) fn respect_plans(&self) -> bool {
        matches!(self, Fetch::Any)
    }

    /// [`crate::usage_store::UsageStore::reserve`]'s `repair_overslept`.
    pub(crate) fn repair_overslept(&self) -> bool {
        match self {
            Fetch::Any => true,
            Fetch::Only { scheduled, .. } => *scheduled,
        }
    }
}

impl Switcher {
    /// `_collect_usage_entries`: one entry per account of `infos`, after one pass.
    ///
    /// # Errors
    /// The usage store's lock or write.
    pub(crate) fn collect_usage_entries(
        &self,
        infos: &[AccountInfo],
        fetch: Fetch<'_>,
    ) -> Result<BTreeMap<String, UsageEntry>> {
        let store = &self.usage_store;
        let identities: Identities = infos
            .iter()
            .map(|info| {
                let identity = (info.email().to_owned(), info.org_uuid().to_owned());
                (info.num().to_owned(), identity)
            })
            .collect();
        // The scoped-window models, so the 429 trust bound honours a per-model reset as the
        // poll planner's view does.
        let inputs = self.poll_policy_inputs();
        let models = inputs.model_names();
        let mut sentinels: BTreeMap<String, String> = infos
            .iter()
            .filter_map(|info| {
                let sentinel = self.static_usage_sentinel(info)?;
                Some((info.num().to_owned(), sentinel.to_owned()))
            })
            .collect();

        let mut entries = store.entries(&identities, &models);
        let now = self.env.now();
        let (respect_plans, repair_overslept) = (fetch.respect_plans(), fetch.repair_overslept());
        let requested: Vec<&str> = infos
            .iter()
            .map(AccountInfo::num)
            .filter(|num| !sentinels.contains_key(*num) && fetch.allows(num))
            .filter(|num| {
                entries
                    .get(*num)
                    .is_none_or(|entry| entry.eligible(now, respect_plans, repair_overslept))
            })
            .collect();
        let claims = store.reserve(&requested, &identities, respect_plans, repair_overslept)?;
        // An expired active credential the pass can't reach (backoff, another collector's
        // lease, a plan) still says so, so the engine holds instead of counting the gap
        // toward a failover. A hold for another machine's reading leaves the row trusted.
        for info in infos.iter().filter(|info| info.is_active()) {
            if sentinels.contains_key(info.num())
                || claims.contains_key(info.num())
                || entries.get(info.num()).is_some_and(|entry| entry.held(now))
            {
                continue;
            }
            if extract_oauth_data(&info.creds)
                .is_some_and(|oauth| is_oauth_token_expired(oauth.get("expiresAt"), now))
            {
                sentinels.insert(info.num().to_owned(), USAGE_TOKEN_EXPIRED.to_owned());
            }
        }

        if !claims.is_empty() {
            let claimed: Vec<&AccountInfo> = infos
                .iter()
                .filter(|info| claims.contains_key(info.num()))
                .collect();
            let records = self.run_usage_fetches(&claimed, &entries);
            let plans = self.plans_after_fetch(&records, &entries, infos, &inputs);
            let accepted = store.record(&records, &identities, Some(&claims), Some(&plans))?;
            for (num, record) in records {
                if let Some(sentinel) = record.sentinel.filter(|_| accepted.contains(&num)) {
                    sentinels.insert(num, sentinel);
                }
            }
            entries = store.entries(&identities, &models);
        }

        Ok(infos
            .iter()
            .map(|info| {
                let entry = entries.remove(info.num()).unwrap_or_default();
                let sentinel = sentinels.get(info.num()).map(String::as_str);
                (info.num().to_owned(), with_sentinel(entry, sentinel))
            })
            .collect())
    }

    /// `_plans_after_fetch`: the cadence each successful fetch commits with its measurement,
    /// planned on the pass's `inputs`. Failures are paced by the store's backoff and keep their
    /// plan for when it lifts.
    pub(crate) fn plans_after_fetch(
        &self,
        records: &BTreeMap<String, FetchRecord>,
        pre: &BTreeMap<String, UsageEntry>,
        infos: &[AccountInfo],
        inputs: &PollInputs,
    ) -> BTreeMap<String, PollPlan> {
        let now = self.env.now();
        let models = inputs.model_names();
        let mut plans = BTreeMap::new();
        for (num, record) in records {
            if record.sentinel.is_some() || record.error.is_some() {
                continue;
            }
            let before = pre.get(num);
            let (next_poll, interval) = plan_after_fetch(&PlanInput {
                prev_interval_s: before.and_then(|entry| entry.poll_interval_s),
                prev_usage: before.and_then(|entry| entry.last_good.as_ref()),
                new_usage: record.usage.as_ref(),
                is_active: infos
                    .iter()
                    .any(|info| info.num() == num && info.is_active()),
                threshold: inputs.threshold,
                models: &models,
                recent_429: before.is_some_and(|entry| entry.recent_429(now)),
                now,
                weekly_threshold: inputs.weekly_threshold,
                rng: random_unit(),
            });
            plans.insert(num.clone(), (Some(next_poll), Some(interval)));
        }
        plans
    }
}

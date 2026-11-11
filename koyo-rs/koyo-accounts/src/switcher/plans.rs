//! Port of claude-swap's `switcher.py` (0fe22a8), the subscription plans of the managed
//! accounts (#350): `_subscription_store`, `_stored_subscription_plans`, `_plans_for_pass`,
//! `refresh_subscription_plans` and `_collect_subscription_plans`.
//!
//! Independent of usage collection by construction: its own file, its own clock, and tried
//! whether or not the slot's usage call just succeeded (a slot in usage backoff is exactly the
//! one whose plan may have changed). Only an access token already valid is used, never
//! refreshed; a due slot without one keeps its record and is tried on the next pass. A read
//! that fails, or that answers for another account, records when it may be repeated and
//! changes nothing else (`subscription::PlanFetch::update`). The network runs with no lock
//! held; only the save takes the plan file's lock. The slot's uuid is the roster's the pass
//! was built on, where cswap reads the roster again.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::subscription::{
    PLAN_TIMEOUT, PLAN_TTL_S, PlanStore, fetch_plan, identity_key, is_due, usable_access_token,
    valid_record,
};
use crate::switcher::info::AccountInfo;
use crate::switcher::{Fetch, Switcher};
use crate::{AccountsError, Result};

/// Each slot's plan record of `records` as stored, those that are valid.
fn stored_plans(infos: &[AccountInfo], records: &Map<String, Value>) -> BTreeMap<String, Value> {
    infos
        .iter()
        .filter_map(|info| {
            let found = records.get(&identity_key(info.email(), Some(info.org_uuid())))?;
            valid_record(found).then(|| (info.num().to_owned(), found.clone()))
        })
        .collect()
}

impl Switcher {
    /// `_subscription_store`: the plan file beside the usage table.
    pub(super) fn subscription_store(&self) -> PlanStore {
        PlanStore::new(&self.env)
    }

    /// `_stored_subscription_plans`: each slot's plan record as stored, those that are valid.
    /// Touches no credential and no network.
    pub(crate) fn stored_subscription_plans(
        &self,
        infos: &[AccountInfo],
    ) -> BTreeMap<String, Value> {
        stored_plans(infos, &self.subscription_store().load())
    }

    /// `_plans_for_pass`: a pass that fetches nothing makes no plan request either.
    ///
    /// # Errors
    /// As [`Switcher::collect_subscription_plans`].
    pub(crate) fn plans_for_pass(
        &self,
        infos: &[AccountInfo],
        fetch: Fetch<'_>,
    ) -> Result<BTreeMap<String, Value>> {
        if fetch.fetches_nothing() {
            return Ok(self.stored_subscription_plans(infos));
        }
        self.collect_subscription_plans(infos)
    }

    /// cswap's `refresh_subscription_plans`: refreshes the due plans, for a view whose
    /// snapshots read the store only.
    ///
    /// # Errors
    /// As [`Switcher::roster`].
    pub fn refresh_subscription_plans(&self) -> Result<()> {
        let infos = self.build_accounts_info()?;
        self.collect_subscription_plans(&infos).map(drop)
    }

    /// `_collect_subscription_plans`: each slot's plan record, refreshed on its own clock;
    /// only slots with a usable record are returned.
    ///
    /// # Errors
    /// A save the guard refused (a lock that stayed held or a failed write is logged, as
    /// cswap's).
    pub(crate) fn collect_subscription_plans(
        &self,
        infos: &[AccountInfo],
    ) -> Result<BTreeMap<String, Value>> {
        let store = self.subscription_store();
        let records = store.load();
        let mut updates = Map::new();
        for info in infos {
            let key = identity_key(info.email(), Some(info.org_uuid()));
            if !is_due(records.get(&key), self.env.now(), PLAN_TTL_S) {
                continue;
            }
            let mut token = usable_access_token(Some(&info.creds), self.env.now());
            if token.is_none() && !info.is_active() {
                // The accounts info was read before the usage pass, which may have changed
                // this slot's backup since; a backup write drops the display cache, so the
                // cached read sees the new token.
                let backup = self
                    .store
                    .read_account_credentials(info.num(), info.email(), true);
                token = usable_access_token(Some(&backup), self.env.now());
            }
            let Some(token) = token else {
                continue;
            };
            let started_at = self.env.now();
            let fetched = fetch_plan(&self.env, &token, PLAN_TIMEOUT);
            let update = fetched.update(started_at, &info.slot.identity.uuid, info.org_uuid());
            updates.insert(key, update);
        }
        if updates.is_empty() {
            return Ok(stored_plans(infos, &records));
        }
        match store.save(&updates) {
            Ok(()) => {}
            Err(error @ (AccountsError::Io(..) | AccountsError::Lock(_))) => {
                log::debug!("Could not save subscription plans: {error}");
            }
            Err(error) => return Err(error),
        }
        Ok(self.stored_subscription_plans(infos))
    }
}

//! The fake switcher the engine's tests run on: cswap's `EngineHarness` seeds a real switcher
//! on its file backend and patches its collector; this one holds the accounts, their
//! credentials, the entries each pass returns and scripted grant outcomes, and logs every
//! fetch, grant and switch.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::json;

use super::super::EngineHost;
use crate::env::Clock as _;
use crate::oauth::RefreshOutcome;
use crate::switcher::{AccountRef, Roster, RosterSlot, SlotIdentity, SwitchReason, SwitchResult};
use crate::testing::{ManualClock, locked};
use crate::usage_store::UsageEntry;
use crate::{AccountsError, Result};

/// One seeded slot.
#[derive(Clone, Debug, Default)]
pub(crate) struct Account {
    pub email: String,
    pub credentials: String,
    pub uuid: String,
    pub org: String,
    pub disabled: bool,
}

impl Account {
    fn identity(&self) -> SlotIdentity {
        SlotIdentity {
            email: self.email.clone(),
            organization_uuid: self.org.clone(),
            uuid: self.uuid.trim().to_owned(),
        }
    }
}

/// Calls that fail.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Fails {
    /// The collector: the store unreadable.
    pub entries: bool,
    /// The roster: the store unreadable.
    pub roster: bool,
    /// The uuid backfill: the store's lock held.
    pub backfill: bool,
}

/// What the fake switcher holds and logs.
#[derive(Default)]
pub(crate) struct Fake {
    pub accounts: BTreeMap<u32, Account>,
    /// The live login's slot.
    pub current: Option<String>,
    /// A live login no slot manages.
    pub unmanaged_live: bool,
    /// The roster names no current account whatever the live login.
    pub hide_current: bool,
    /// What every collection pass returns.
    pub entries: BTreeMap<String, UsageEntry>,
    /// Each pass's `(fetch, scheduled)`.
    pub fetches: Vec<(BTreeSet<String>, bool)>,
    /// Scripted grant outcomes per slot; an unscripted grant succeeds with a token good for 8 h.
    pub grants: BTreeMap<String, VecDeque<RefreshOutcome>>,
    pub grant_log: Vec<String>,
    pub switch_log: Vec<String>,
    /// A `switch_to` that answers this instead of switching.
    pub switch_answer: Option<SwitchResult>,
    /// Each threshold the engine pinned.
    pub poll_thresholds: Vec<f64>,
    /// Whether the engine unpinned it.
    pub poll_unpinned: bool,
    pub backfills: Vec<(String, String)>,
    pub fail: Fails,
    /// Seconds the clock moves when a credential is read (the freshen loop, after the ranking).
    pub advance_on_read: Option<f64>,
}

impl Fake {
    fn account(&self, number: &str) -> Option<&Account> {
        number
            .parse()
            .ok()
            .and_then(|num: u32| self.accounts.get(&num))
    }

    fn account_mut(&mut self, number: &str) -> Option<&mut Account> {
        number
            .parse()
            .ok()
            .and_then(|num: u32| self.accounts.get_mut(&num))
    }

    /// The live login's identity: the current slot's, a stranger's for an unmanaged one.
    fn live(&self) -> Option<SlotIdentity> {
        let managed = self
            .current
            .as_deref()
            .and_then(|current| self.account(current));
        (self.current.is_some() || self.unmanaged_live).then(|| {
            managed.map_or_else(
                || SlotIdentity {
                    email: "stranger@example.com".to_owned(),
                    ..SlotIdentity::default()
                },
                Account::identity,
            )
        })
    }

    fn email(&self, number: &str) -> String {
        self.account(number)
            .map(|account| account.email.clone())
            .unwrap_or_default()
    }
}

pub(crate) struct FakeHost {
    pub fake: Mutex<Fake>,
    clock: Arc<ManualClock>,
}

impl FakeHost {
    pub fn new(clock: Arc<ManualClock>) -> FakeHost {
        FakeHost {
            fake: Mutex::new(Fake::default()),
            clock,
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Fake> {
        locked(&self.fake)
    }
}

pub(crate) fn credentials(num: &str, expires_at: Option<f64>) -> String {
    let mut oauth =
        json!({"accessToken": format!("sk-{num}"), "refreshToken": format!("rt-{num}")});
    if let Some(expires_at) = expires_at {
        oauth["expiresAt"] = json!(expires_at);
    }
    json!({"claudeAiOauth": oauth}).to_string()
}

impl EngineHost for FakeHost {
    fn usage_entries_by_account(
        &self,
        fetch: &BTreeSet<String>,
        scheduled: bool,
    ) -> Result<BTreeMap<String, UsageEntry>> {
        let mut fake = self.lock();
        fake.fetches.push((fetch.clone(), scheduled));
        if fake.fail.entries {
            return Err(AccountsError::Config("store unreadable".into()));
        }
        Ok(fake.entries.clone())
    }

    fn roster(&self) -> Result<Roster> {
        let fake = self.lock();
        if fake.fail.roster {
            return Err(AccountsError::Config("sequence unreadable".into()));
        }
        let current = fake
            .current
            .clone()
            .filter(|current| !fake.hide_current && fake.account(current).is_some());
        Ok(Roster {
            current,
            live: fake.live(),
            slots: fake
                .accounts
                .iter()
                .map(|(num, account)| RosterSlot {
                    number: num.to_string(),
                    identity: account.identity(),
                    organization_name: String::new(),
                    has_backup: !account.credentials.is_empty(),
                    disabled: account.disabled,
                })
                .collect(),
        })
    }

    fn backfill_account_uuid(&self, number: &str, slot: &SlotIdentity, uuid: &str) -> Result<()> {
        let mut fake = self.lock();
        fake.backfills.push((number.to_owned(), uuid.to_owned()));
        if fake.fail.backfill {
            return Err(AccountsError::Lock("held".into()));
        }
        if let Some(account) = fake.account_mut(number)
            && account.uuid.is_empty()
            && account.email == slot.email
        {
            account.uuid = uuid.to_owned();
        }
        Ok(())
    }

    fn set_poll_threshold(&self, threshold: f64) {
        self.lock().poll_thresholds.push(threshold);
    }

    fn clear_poll_threshold(&self) {
        self.lock().poll_unpinned = true;
    }

    fn read_account_credentials(&self, number: &str, _email: &str) -> String {
        if let Some(seconds) = self.lock().advance_on_read {
            self.clock.advance(seconds);
        }
        self.lock()
            .account(number)
            .map(|account| account.credentials.clone())
            .unwrap_or_default()
    }

    fn consume_backup_grant(&self, number: &str, _email: &str, _snapshot: &str) -> RefreshOutcome {
        let now_ms = self.clock.now() * 1000.;
        let mut fake = self.lock();
        fake.grant_log.push(number.to_owned());
        let outcome = fake
            .grants
            .get_mut(number)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| RefreshOutcome {
                credentials: Some(credentials(number, Some(now_ms + 8. * 3600. * 1000.))),
                ..RefreshOutcome::default()
            });
        // The gate persists a successor itself.
        if let (Some(fresh), Some(account)) =
            (outcome.credentials.as_ref(), fake.account_mut(number))
        {
            account.credentials.clone_from(fresh);
        }
        outcome
    }

    fn switch_to(&self, number: &str) -> Result<SwitchResult> {
        let mut fake = self.lock();
        fake.switch_log.push(number.to_owned());
        if let Some(answer) = fake.switch_answer.clone() {
            return Ok(answer);
        }
        let from = fake
            .current
            .clone()
            .map(|current| AccountRef::new(current.parse().ok(), &fake.email(&current)));
        let to = AccountRef::new(number.parse().ok(), &fake.email(number));
        fake.current = Some(number.to_owned());
        Ok(SwitchResult {
            switched: true,
            from,
            to: Some(to),
            reason: SwitchReason::Switched,
            message: String::new(),
            warnings: Vec::new(),
        })
    }
}

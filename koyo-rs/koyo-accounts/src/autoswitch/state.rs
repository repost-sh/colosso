//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): the engine's state file,
//! `autoswitch_state.json` in Koyo's store, read-modify-written under its own lock
//! (`.autoswitch_state.lock` beside it) so two engines never overwrite each other's quarantine
//! or cooldown. Its keys and schema are cswap's: `schemaVersion`, `quarantine` (slot →
//! `email`, `reason`, `at`, `refreshTokenFingerprint`), the last switch (`lastSwitchAt`,
//! `lastSwitchTo`, `lastSwitchFrom`, `leftHeadroom`, `leftRecoveryAt`, `leftTrigger`) and the
//! health counters (`unhealthyTicks`, `idleHoldSince`, `healthAccount`, `healthTickAt`).
//!
//! Also here: quarantine and its release, the health counters' load and save, and the
//! cooldown.
//!
//! Read as cswap reads its dict: a value of the wrong type is absent, and an unreadable file is
//! `{}`. Where cswap writes a null (an unknown headroom, an `inf` recovery, no account left),
//! the key is left out, which every reader here takes the same way; keys this engine doesn't
//! know aren't kept.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value};

use crate::locking::{DEFAULT_TIMEOUT, FileLock};
use crate::oauth::{credential_fingerprint, iso_utc};
use crate::switcher::Roster;
use crate::{Result, fsutil, paths};

use super::events::{
    AutoSwitchEvent, QuarantineEvent, QuarantineReason, ReleaseReason, Trigger, UnquarantineEvent,
};
use super::{AutoSwitchEngine, IDLE_HOLD_MAX_S};

pub(crate) const STATE_SCHEMA_VERSION: i64 = 1;
/// The state file's lock, beside it.
const STATE_LOCK_FILENAME: &str = ".autoswitch_state.lock";

/// The state file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct AutoSwitchState {
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<i64>,
    #[serde(
        deserialize_with = "lenient",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub quarantine: BTreeMap<String, QuarantineEntry>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub last_switch_at: Option<f64>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub last_switch_to: Option<StoredSlot>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub last_switch_from: Option<StoredSlot>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub left_headroom: Option<f64>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub left_recovery_at: Option<f64>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub left_trigger: Option<Trigger>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub unhealthy_ticks: Option<i64>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub idle_hold_since: Option<f64>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub health_account: Option<String>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub health_tick_at: Option<f64>,
}

/// One quarantined slot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct QuarantineEntry {
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// [`QuarantineReason::as_str`].
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub refresh_token_fingerprint: Option<String>,
}

/// A slot number as stored: `lastSwitchTo` as a string, `lastSwitchFrom` as the switch result's
/// number; compared as text (cswap's `str(value)`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum StoredSlot {
    Number(Number),
    Text(String),
}

impl StoredSlot {
    pub fn text(&self) -> String {
        match self {
            StoredSlot::Number(number) => number.to_string(),
            StoredSlot::Text(text) => text.clone(),
        }
    }
}

/// A slot quarantined this tick, with the fingerprint of the credential the store holds for it.
pub(crate) struct Quarantined {
    pub number: String,
    pub email: String,
    pub reason: QuarantineReason,
    pub fingerprint: Option<String>,
}

/// A field's value, its default when the file holds another type there.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(
    deserializer: D,
) -> std::result::Result<T, D::Error> {
    Value::deserialize(deserializer).map(|value| serde_json::from_value(value).unwrap_or_default())
}

impl AutoSwitchState {
    pub fn in_cooldown(&self, now: f64, cooldown_seconds: f64) -> bool {
        self.last_switch_at
            .is_some_and(|last| now - last < cooldown_seconds)
    }

    pub fn clear_health(&mut self) {
        self.unhealthy_ticks = None;
        self.idle_hold_since = None;
        self.health_account = None;
        self.health_tick_at = None;
    }
}

impl AutoSwitchEngine {
    /// The state, `{}` when the file is missing, unreadable or not an object.
    pub(crate) fn read_state(&self) -> AutoSwitchState {
        std::fs::read_to_string(paths::store::autoswitch_state(&self.env))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Locks, reads, runs `run` on the state and writes it back, cswap's `_mutate_state` and
    /// `_perform`'s locked block. Never called with another lock held; an error from `run`
    /// writes nothing.
    pub(crate) fn with_state<T>(
        &self,
        run: impl FnOnce(&mut AutoSwitchState) -> Result<T>,
    ) -> Result<T> {
        let path = paths::store::autoswitch_state(&self.env);
        let _lock = FileLock::hold(
            &self.env,
            &path.with_file_name(STATE_LOCK_FILENAME),
            DEFAULT_TIMEOUT,
        )?;
        let mut state = self.read_state();
        state.schema_version = Some(STATE_SCHEMA_VERSION);
        let value = run(&mut state)?;
        fsutil::atomic_write_json(&self.env, &path, &state)?;
        Ok(value)
    }

    /// Writes this tick's quarantines in one go, then reports them.
    pub(crate) fn quarantine(&mut self, quarantined: Vec<Quarantined>) -> Result<()> {
        if quarantined.is_empty() {
            return Ok(());
        }
        let at = iso_utc(self.now());
        self.with_state(|state| {
            for slot in &quarantined {
                state.quarantine.insert(
                    slot.number.clone(),
                    QuarantineEntry {
                        email: Some(slot.email.clone()),
                        reason: Some(slot.reason.as_str().to_owned()),
                        at: at.clone(),
                        refresh_token_fingerprint: slot.fingerprint.clone(),
                    },
                );
            }
            Ok(())
        })?;
        for slot in quarantined {
            self.emit(AutoSwitchEvent::Quarantine(QuarantineEvent {
                ts: self.now(),
                number: slot.number,
                email: slot.email,
                reason: slot.reason,
            }));
        }
        Ok(())
    }

    /// Drops quarantine entries whose credential was replaced since: a changed refresh-token
    /// fingerprint (or a removed and re-added slot) means the person signed in again and
    /// re-added the account, so the dead lineage is gone and it re-enters rotation.
    pub(crate) fn release_recovered_quarantines(
        &mut self,
        roster: &Roster,
        state: &mut AutoSwitchState,
    ) -> Result<()> {
        let mut released: Vec<(String, String, ReleaseReason)> = Vec::new();
        for (number, entry) in &state.quarantine {
            let email_now = roster.email(number);
            if email_now.is_empty() || Some(email_now) != entry.email.as_deref() {
                let email = entry.email.clone().unwrap_or_default();
                released.push((number.clone(), email, ReleaseReason::AccountReplaced));
                continue;
            }
            let credentials = self.host.read_account_credentials(number, email_now);
            if credential_fingerprint(&credentials) != entry.refresh_token_fingerprint {
                let email = email_now.to_owned();
                released.push((number.clone(), email, ReleaseReason::CredentialsReplaced));
            }
        }
        if released.is_empty() {
            return Ok(());
        }
        *state = self.with_state(|fresh| {
            for (number, _, _) in &released {
                fresh.quarantine.remove(number);
            }
            Ok(fresh.clone())
        })?;
        for (number, email, reason) in released {
            self.emit(AutoSwitchEvent::Unquarantine(UnquarantineEvent {
                ts: self.now(),
                number,
                email,
                reason,
            }));
        }
        Ok(())
    }

    /// Seeds the unreadable-active counters from the state file.
    ///
    /// They persist so a fresh engine keeps counting toward failover instead of restarting at
    /// zero. Counters left by another account, or by a tick longer ago than twice the idle-hold
    /// cap, are dropped. Until the managed active account changes the in-memory values rule, so
    /// long gaps between ticks keep their progress.
    pub(crate) fn load_health_counters(&mut self, state: &AutoSwitchState, current: &str) {
        let now = self.now();
        let fresh = state.health_account.as_deref() == Some(current)
            && state
                .health_tick_at
                .is_some_and(|at| (0. ..=2. * IDLE_HOLD_MAX_S).contains(&(now - at)));
        self.unhealthy_ticks = state
            .unhealthy_ticks
            .filter(|ticks| fresh && *ticks > 0)
            .unwrap_or(0);
        self.idle_hold_since = state.idle_hold_since.filter(|_| fresh);
    }

    /// Saves the counters, or clears them once the tick's read (`state`) holds some and nothing
    /// is counting.
    pub(crate) fn persist_health_counters(&self, state: &AutoSwitchState) -> Result<()> {
        let (ticks, since) = (self.unhealthy_ticks, self.idle_hold_since);
        let counting = ticks != 0 || since.is_some();
        if !counting && state.health_account.is_none() {
            return Ok(());
        }
        let current = self.health_account.clone().filter(|_| counting);
        let now = self.now();
        self.with_state(|fresh| {
            match current {
                Some(current) => {
                    fresh.unhealthy_ticks = Some(ticks);
                    fresh.idle_hold_since = since;
                    fresh.health_account = Some(current);
                    fresh.health_tick_at = Some(now);
                }
                None => fresh.clear_health(),
            }
            Ok(())
        })
    }
}

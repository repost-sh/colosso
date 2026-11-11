//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): the switch, once a target is
//! chosen. Each ranked candidate's token is freshened through the switcher's refresh gate before
//! it's activated (`_freshen_target`), the identity the token endpoint sends checks the slot
//! (`_note_token_identity`), a dead or wrong-account target is quarantined, and the first that
//! freshens is switched to under the state lock (`_perform`).
//!
//! cswap's freshen also answered `ok` for an API key and `skip-live-session` for a slot a live
//! `cswap run` session owned; Koyo has neither. cswap writes each quarantine as it's found;
//! here a tick's are written together before the switch, with the fingerprint of the credential
//! the store holds for each (the successor the gate persisted, or the generation it sent), so
//! nothing is read again.

use crate::Result;
use crate::oauth::value::py_number;
use crate::oauth::{AccountIdentity, credential_fingerprint, expires_within, extract_oauth_data};
use crate::switcher::{Roster, SlotIdentity, SwitchReason, SwitchResult};
use crate::usage_store::PERMANENT_AUTH_ERRORS;

use super::events::{
    AutoSwitchEvent, NoSwitchReason, QuarantineReason, SwitchEvent, TickOutcome, Trigger,
};
use super::state::{AutoSwitchState, Quarantined, StoredSlot};
use super::{ActivePlan, AutoSwitchEngine, FRESHEN_BUFFER_MS};

/// The systemic freshen refusals, most actionable first: deterministic conditions every
/// candidate hits alike, so a tick reports one, and the order decides which, since naming the
/// cause that clears itself hides the one that needs a person. `store-unmirrored` and
/// `invalid_client` stay until someone unsets a variable or fixes a client registration,
/// `stash-unreadable` until they unlock a keychain or fix the file; `consume-busy` is gone by
/// the next pass. They're the gate's `oauth::DETERMINISTIC_REFRESH_ERRORS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Systemic {
    StoreUnmirrored,
    InvalidClient,
    StashUnreadable,
    ConsumeBusy,
}

impl Systemic {
    /// The gate's error string, [`Systemic::from_kind`] read back: the tests hold the two to
    /// each other and to the gate's list.
    #[cfg(test)]
    pub fn kind(self) -> &'static str {
        match self {
            Systemic::StoreUnmirrored => "store-unmirrored",
            Systemic::InvalidClient => "invalid_client",
            Systemic::StashUnreadable => "stash-unreadable",
            Systemic::ConsumeBusy => "consume-busy",
        }
    }

    pub fn from_kind(kind: &str) -> Option<Systemic> {
        match kind {
            "store-unmirrored" => Some(Systemic::StoreUnmirrored),
            "invalid_client" => Some(Systemic::InvalidClient),
            "stash-unreadable" => Some(Systemic::StashUnreadable),
            "consume-busy" => Some(Systemic::ConsumeBusy),
            _ => None,
        }
    }

    /// What to do about it. cswap's remedies name its own commands; Koyo's don't.
    pub fn remedy(self) -> &'static str {
        match self {
            Systemic::StoreUnmirrored => {
                "CLAUDE_SECURESTORAGE_CONFIG_DIR is set — unset it in the shell Koyo starts from"
            }
            Systemic::InvalidClient => "the OAuth client was rejected — systemic, not this account",
            Systemic::StashUnreadable => {
                "a stashed successor is unreadable — unlock the keychain or fix the file, then retry"
            }
            Systemic::ConsumeBusy => "another refresh holds the slot — retries next pass",
        }
    }
}

/// What freshening a target came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Freshen {
    Ok,
    /// A dead lineage, or alive but authenticating as another account: quarantine, never
    /// activate. Carries the fingerprint to record.
    Quarantine(QuarantineReason, Option<String>),
    /// Network trouble: try again next tick.
    Transient,
    Systemic(Systemic),
}

/// The account being left, as the ranking read it: its headroom and its binding reset, so the
/// next tick can tell whether it recovered (`release.rs`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Departure {
    pub headroom: Option<f64>,
    pub recovery: f64,
}

/// What the locked block of [`AutoSwitchEngine::perform`] came to.
enum Performed {
    Cooldown,
    AlreadyActive(SwitchReason),
    Switched(SwitchResult),
}

impl AutoSwitchEngine {
    /// Freshens and switches to the first candidate that can take over.
    pub(crate) fn switch_to_first(
        &mut self,
        roster: &Roster,
        ordered: &[String],
        trigger: Trigger,
        left: Departure,
        state: &mut AutoSwitchState,
    ) -> Result<TickOutcome> {
        let mut transient_failure = false;
        let mut systemic: Option<Systemic> = None;
        let mut quarantined = Vec::new();
        let mut target = None;
        for num in ordered {
            let identity = roster
                .slot(num)
                .map(|slot| slot.identity.clone())
                .unwrap_or_default();
            match self.freshen_target(num, &identity) {
                Freshen::Quarantine(reason, fingerprint) => quarantined.push(Quarantined {
                    number: num.clone(),
                    email: identity.email,
                    reason,
                    fingerprint,
                }),
                Freshen::Transient => transient_failure = true,
                // One cause is reported, so the one worth acting on: the most actionable.
                Freshen::Systemic(kind) => {
                    systemic = Some(systemic.map_or(kind, |kept| kept.min(kind)));
                }
                Freshen::Ok => {
                    target = Some(num);
                    break;
                }
            }
        }
        self.quarantine(quarantined)?;
        if let Some(num) = target {
            return self.perform(num, trigger, left, state);
        }
        if systemic.is_some() || transient_failure {
            let message = match systemic {
                Some(systemic) => format!("could not freshen: {}", systemic.remedy()),
                None => "could not freshen any candidate (network?)".to_owned(),
            };
            self.emit_error(message);
            return Ok(TickOutcome::Error);
        }
        self.no_switch(NoSwitchReason::NoViableTarget, "");
        Ok(TickOutcome::Blocked)
    }

    /// Makes sure a candidate's stored token outlives Claude Code's 5-minute refresh buffer
    /// before it's activated. Only ever touches the slot's backup: the active credential is
    /// Claude Code's.
    pub(crate) fn freshen_target(&self, number: &str, identity: &SlotIdentity) -> Freshen {
        let email = identity.email.as_str();
        let credentials = self.host.read_account_credentials(number, email);
        if credentials.is_empty() {
            return Freshen::Transient;
        }
        let Some(data) = extract_oauth_data(&credentials).filter(|data| !data.is_empty()) else {
            return Freshen::Quarantine(
                QuarantineReason::InvalidGrant,
                credential_fingerprint(&credentials),
            );
        };
        let expires_at = data.get("expiresAt").and_then(py_number);
        if !expires_within(expires_at, self.now() * 1000., FRESHEN_BUFFER_MS) {
            return Freshen::Ok;
        }
        // The gate serializes every POST of a backup refresh token: it re-reads under the slot
        // lock (this snapshot may be superseded) and persists by a fingerprint compare-and-swap,
        // so a freshen racing the collector can't consume one grant twice.
        let outcome = self.host.consume_backup_grant(number, email, &credentials);
        if outcome.error.is_none()
            && let Some(fresh) = outcome.credentials.as_deref().filter(|c| !c.is_empty())
        {
            // The gate persisted the successor already. A credential that authenticates as
            // another account would put the person on the wrong account with every gauge
            // normal: not a target, quarantined until a re-add replaces it.
            if self.note_token_identity(number, identity, outcome.token_account.as_ref()) {
                return Freshen::Quarantine(
                    QuarantineReason::IdentityConflict,
                    credential_fingerprint(fresh),
                );
            }
            return Freshen::Ok;
        }
        match outcome.error.as_deref() {
            // The gate may have adopted a stashed successor before the POST: the generation it
            // sent is the one the store holds.
            Some(kind) if PERMANENT_AUTH_ERRORS.contains(&kind) => Freshen::Quarantine(
                QuarantineReason::InvalidGrant,
                outcome
                    .consumed_fp
                    .or_else(|| credential_fingerprint(&credentials)),
            ),
            // Deterministic, not network trouble: reported apart so the error names the real
            // cause instead of "(network?)".
            Some(kind) => Systemic::from_kind(kind).map_or(Freshen::Transient, Freshen::Systemic),
            None => Freshen::Transient,
        }
    }

    /// Uses the identity the token endpoint sent (free with the grant) to check or fill in the
    /// slot's. True on a conflict: the credential authenticates under another organization than
    /// the slot records (compared first, whenever both record one) or as another account uuid.
    /// An empty slot uuid is backfilled, but only with no org conflict: a wrong-org credential is
    /// evidence the slot holds the wrong account, and backfilling its uuid would poison the
    /// slot's identity for good (a backfill never rewrites a uuid).
    pub(crate) fn note_token_identity(
        &self,
        number: &str,
        slot: &SlotIdentity,
        token_account: Option<&AccountIdentity>,
    ) -> bool {
        let Some(token_account) = token_account else {
            return false;
        };
        let token_uuid = token_account.uuid.trim();
        if token_uuid.is_empty() {
            return false;
        }
        let token_org = token_account
            .organization_uuid
            .as_deref()
            .unwrap_or_default();
        if !token_org.is_empty()
            && !slot.organization_uuid.is_empty()
            && token_org != slot.organization_uuid
        {
            return true;
        }
        if slot.uuid.is_empty() {
            // Bookkeeping never breaks a freshen.
            if let Err(error) = self.host.backfill_account_uuid(number, slot, token_uuid) {
                log::debug!("uuid backfill failed for account {number}: {error}");
            }
            return false;
        }
        slot.uuid != token_uuid
    }

    /// Switches to `number` and records it. The state lock is held across the cooldown recheck,
    /// the switch and its record, so two engines make one serialized decision: the loser re-reads
    /// the winner's `lastSwitchAt` and backs off. No deadlock cycle: the switch path never takes
    /// the state lock. The record clears the health counters: they counted the account left.
    fn perform(
        &mut self,
        number: &str,
        trigger: Trigger,
        left: Departure,
        state: &mut AutoSwitchState,
    ) -> Result<TickOutcome> {
        let performed = self.with_state(|fresh| {
            if trigger == Trigger::Proactive
                && fresh.in_cooldown(self.now(), self.settings.cooldown_seconds)
            {
                return Ok(Performed::Cooldown);
            }
            let result = self.host.switch_to(number)?;
            if !result.switched {
                return Ok(Performed::AlreadyActive(result.reason));
            }
            // Where we came from, so the next tick can refuse to undo this, and what it looked
            // like, so that refusal has a release burn can't fake (`release.rs`). An `inf`
            // recovery isn't portable JSON: left out. The trigger is recorded so a reader never
            // infers it from the snapshot.
            fresh.last_switch_at = Some(self.now());
            fresh.last_switch_to = Some(StoredSlot::Text(number.to_owned()));
            fresh.last_switch_from = result
                .from
                .as_ref()
                .and_then(|from| from.number)
                .map(|from| StoredSlot::Number(from.into()));
            fresh.left_headroom = left.headroom;
            fresh.left_recovery_at = Some(left.recovery).filter(|at| at.is_finite());
            fresh.left_trigger = Some(trigger);
            fresh.clear_health();
            state.clone_from(fresh);
            Ok(Performed::Switched(result))
        })?;
        let result = match performed {
            Performed::Cooldown => {
                self.no_switch(NoSwitchReason::Cooldown, "");
                return Ok(TickOutcome::NoAction);
            }
            Performed::AlreadyActive(reason) => {
                self.no_switch(NoSwitchReason::AlreadyActive, reason.as_str());
                return Ok(TickOutcome::NoAction);
            }
            Performed::Switched(result) => result,
        };
        self.unhealthy_ticks = 0;
        self.idle_hold_since = None;
        // `switch_to` replanned the new active row: the wait reads that plan.
        self.active_plan = ActivePlan::Replanned;
        self.emit(AutoSwitchEvent::Switch(SwitchEvent {
            ts: self.now(),
            trigger,
            from_ref: result.from,
            to_ref: result.to,
            warnings: result.warnings,
        }));
        Ok(TickOutcome::Switched)
    }
}

//! Port of claude-swap's `switcher.py` (0fe22a8), switching to a named slot: `switch_to` with
//! its already-active short-circuit, the pre-lock resolution of the live credential's owner,
//! and the result. The switch itself is in `perform.rs` (normal path) and `direct.rs` (direct
//! activation), the outgoing credential's verdict in `classify.rs`, the safety copy and the
//! foreign-successor rescue in `rescue.rs`, the transaction in `rollback.rs`.
//!
//! A self-switch is provenance-aware (cswap issues #79 and #117): it is a no-op when the live
//! credential is the slot's stored lineage, or when it diverged and its owner can't be
//! resolved (endpoint trouble must never surface here, and leaving everything untouched is the
//! safe write); a divergence resolved over the network runs the full switch, so the outgoing
//! backup can be re-synced or the foreign bytes preserved. The owner's resolution reuses what
//! the self-switch check read (the live credential, the identity, the slot and its backup)
//! rather than reading them again as cswap's `_prefetch_live_identity` does.
//!
//! Left out: the bare `switch()` and its strategies, `--force`, the human-mode printing and
//! the post-switch `list_accounts`, aliases and the interactive choice between slots sharing an
//! email.

use crate::oauth::{self, AccountIdentity, extract_access_token};
use crate::poll_policy::MIN_INTERVAL_S;
use crate::switcher::Switcher;
use crate::switcher::identity::same_lineage;
use crate::switcher::identity::{is_digits, validate_email};
use crate::switcher::store::{record, slot_int, text};
use crate::{AccountsError, Result};

mod classify;
mod direct;
mod perform;
mod rescue;
pub(crate) mod rollback;

pub(crate) use classify::{ForeignSlot, OutgoingKind};

/// cswap's `account_ref`: a slot number (`None` for an unmanaged login) and its email.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRef {
    pub number: Option<u64>,
    pub email: String,
}

impl AccountRef {
    pub fn new(number: Option<u64>, email: &str) -> AccountRef {
        AccountRef {
            number,
            email: email.to_owned(),
        }
    }
}

/// Why a switch result is what it is: cswap's `reason` strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchReason {
    /// The live identity changed.
    Switched,
    /// The target was already the live account.
    AlreadyActive,
}

impl SwitchReason {
    /// cswap's string: `"switched"` or `"already-active"`.
    pub fn as_str(self) -> &'static str {
        match self {
            SwitchReason::Switched => "switched",
            SwitchReason::AlreadyActive => "already-active",
        }
    }
}

/// What a switch did, as cswap's `--json` switch result carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchResult {
    /// The live identity changed (`from != to`).
    pub switched: bool,
    /// The account left: `None` on a machine with no live login.
    pub from: Option<AccountRef>,
    pub to: Option<AccountRef>,
    pub reason: SwitchReason,
    pub message: String,
    pub warnings: Vec<String>,
}

/// `_prefetch_live_identity`'s answer: the live bytes read before the locks and, when they
/// diverged from the slot's backup, whose token they are. `resolved` holds only while the live
/// bytes haven't moved; the classifier re-checks them under the lock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Provenance {
    pub live: Option<String>,
    pub resolved: Option<AccountIdentity>,
}

impl Provenance {
    /// Live bytes whose owner wasn't (or needn't be) resolved.
    fn unresolved(live: String) -> Provenance {
        Provenance {
            live: Some(live),
            resolved: None,
        }
    }
}

/// The slot a switch leaves (cswap's `current_account`, `current_email`) and the pre-lock
/// provenance of its live credential.
pub(crate) struct Outgoing<'a> {
    pub account: &'a str,
    pub email: &'a str,
    pub provenance: &'a Provenance,
}

/// `_perform_switch`'s return: the identities left and landed, captured under the lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SwitchOp {
    pub from: Option<AccountRef>,
    pub to: AccountRef,
    pub warnings: Vec<String>,
}

/// `_live_matches_slot_backup`'s answer, with what it read for the owner's resolution.
enum LiveVsBackup {
    /// Provably the slot's stored lineage, or no live credential to judge.
    Matches,
    /// The live credential and the slot's backup it diverged from.
    Diverged { live: String, backup: String },
}

impl Switcher {
    /// `switch_to`: switches to the slot `identifier` names (a number, or an email one slot
    /// holds).
    ///
    /// # Errors
    /// [`AccountsError::Config`] when no account is managed yet (or the email is ambiguous),
    /// [`AccountsError::Validation`] for an identifier that is neither a number nor an email,
    /// [`AccountsError::AccountNotFound`]; the switch's own (`perform_switch`), each with
    /// nothing changed or rolled back.
    pub fn switch_to(&self, identifier: &str) -> Result<SwitchResult> {
        if !self.sequence_file.exists() {
            return Err(AccountsError::Config("No accounts are managed yet".into()));
        }
        if !is_digits(identifier) && !validate_email(identifier) {
            return Err(AccountsError::Validation(format!(
                "Invalid account identifier: {identifier}"
            )));
        }
        let Some(target) = self.resolve_account_identifier(identifier)? else {
            return Err(AccountsError::AccountNotFound(format!(
                "No account found with identifier: {identifier}"
            )));
        };
        let data = self.sequence_data_or_empty()?;
        if record(&data, &target).is_none() {
            return Err(AccountsError::AccountNotFound(format!(
                "Account-{target} does not exist"
            )));
        }
        // A self-switch would back the live credential up into the target slot, then read it
        // straight back: destroying a fresher backup with a possibly stale login (#79).
        let mut provenance = None;
        if let Some((email, org)) = self.get_current_account()
            && Switcher::find_account_slot(&data, &email, &org).as_deref() == Some(&target)
        {
            provenance = self.self_switch_action(&target, &email);
            if provenance.is_none() {
                let email = text(record(&data, &target), "email");
                let reference = AccountRef::new(Some(slot_int(&target)?), email);
                return Ok(Switcher::switch_noop(
                    format!("Already on Account-{target} ({email})"),
                    Some(reference),
                ));
            }
        }
        let op = self.perform_switch(&target, provenance)?;
        Ok(Switcher::switch_result_from_op(op))
    }

    /// `_switch_result_from_op`: `switched` is whether the live identity changed.
    pub(crate) fn switch_result_from_op(op: SwitchOp) -> SwitchResult {
        let switched = op.from.as_ref() != Some(&op.to);
        let label = format!(
            "Account-{} ({})",
            op.to
                .number
                .map_or_else(|| "None".to_owned(), |number| number.to_string()),
            op.to.email
        );
        let (reason, message) = if switched {
            (SwitchReason::Switched, format!("Switched to {label}"))
        } else {
            (SwitchReason::AlreadyActive, format!("Already on {label}"))
        };
        SwitchResult {
            switched,
            from: op.from,
            to: Some(op.to),
            reason,
            message,
            warnings: op.warnings,
        }
    }

    /// `_switch_noop`: a result that switched nothing, the target being already active;
    /// `from` is where the person stayed.
    pub(crate) fn switch_noop(message: String, to: Option<AccountRef>) -> SwitchResult {
        SwitchResult {
            switched: false,
            from: to.clone(),
            to,
            reason: SwitchReason::AlreadyActive,
            message,
            warnings: Vec::new(),
        }
    }

    /// `_live_matches_slot_backup`: whether the live credential is provably the slot's stored
    /// lineage (bytes or refresh-token fingerprint). An unreadable or empty live credential
    /// keeps the no-op: forcing a switch on missing evidence would fail later anyway.
    fn live_matches_slot_backup(&self, slot: &str, email: &str) -> LiveVsBackup {
        let Some(live) = self
            .store
            .read_credentials()
            .filter(|live| !live.is_empty())
        else {
            return LiveVsBackup::Matches;
        };
        let backup = self.store.read_account_credentials(slot, email, false);
        if !backup.is_empty() && same_lineage(&live, &backup) {
            return LiveVsBackup::Matches;
        }
        LiveVsBackup::Diverged { live, backup }
    }

    /// `_self_switch_action`, as the module says: the provenance to reconcile, or `None` for
    /// the no-op (the live credential is the slot's backup, or it diverged and couldn't be
    /// classified, which is silent).
    fn self_switch_action(&self, slot: &str, email: &str) -> Option<Provenance> {
        let LiveVsBackup::Diverged { live, backup } = self.live_matches_slot_backup(slot, email)
        else {
            return None;
        };
        let provenance = self.resolve_live_owner(live, &backup);
        if provenance.resolved.is_none() {
            log::info!(
                "Live credential diverges from Account-{slot}'s stored backup and ownership \
                 could not be verified; self-switch left everything untouched."
            );
            return None;
        }
        Some(provenance)
    }

    /// `_prefetch_live_identity`: the live credential's owner resolved BEFORE the locks are
    /// taken (no network under them). When the live bytes or lineage match the slot the config
    /// names, the answer is local and nothing is fetched. The oracle is advisory: a failure is
    /// `resolved: None`, never an error.
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`]: a torn roster.
    pub(crate) fn prefetch_live_identity(&self) -> Result<Provenance> {
        let Some(live) = self.store.read_credentials() else {
            log::debug!("Pre-lock live credential read failed");
            return Ok(Provenance::default());
        };
        if live.is_empty() {
            return Ok(Provenance::unresolved(live));
        }
        let Some((email, org)) = self.get_current_account() else {
            return Ok(Provenance::unresolved(live));
        };
        let data = self.sequence_data_or_empty()?;
        let Some(slot) = Switcher::find_account_slot(&data, &email, &org) else {
            return Ok(Provenance::unresolved(live));
        };
        let backup = self.store.read_account_credentials(&slot, &email, false);
        Ok(self.resolve_live_owner(live, &backup))
    }

    /// The prefetch's tail, once the slot the config names and its backup are read: local when
    /// the live credential is that backup's lineage, else the profile oracle's answer for its
    /// access token.
    fn resolve_live_owner(&self, live: String, backup: &str) -> Provenance {
        if same_lineage(backup, &live) {
            // Provenance already established locally.
            return Provenance::unresolved(live);
        }
        // A raw API key or garbled JSON has no token to resolve.
        let resolved = extract_access_token(&live)
            .filter(|token| !token.is_empty())
            .and_then(|token| oauth::fetch_oauth_profile(&self.env, &token));
        Provenance {
            live: Some(live),
            resolved,
        }
    }

    /// `_replan_new_active`: pulls the just-activated account's poll plan to the active floor.
    /// Its plan was computed while it idled as a candidate and may wait the candidate maximum.
    /// The deadline anchors on the last measurement (an old one comes due now; an account never
    /// measured is left plan-less, so nothing blocks its first fetch), and a plan is only ever
    /// pulled earlier (`UsageStore::pull_plan_earlier`). Best-effort: the switch has committed,
    /// so a failure here is logged.
    pub(crate) fn replan_new_active(&self, number: &str, email: &str, org_uuid: &str) {
        let identity = (email.to_owned(), org_uuid.to_owned());
        if let Err(error) = self
            .usage_store
            .pull_plan_earlier(number, &identity, MIN_INTERVAL_S)
        {
            log::warn!("Post-switch poll re-plan failed (switch itself succeeded): {error}");
        }
    }
}

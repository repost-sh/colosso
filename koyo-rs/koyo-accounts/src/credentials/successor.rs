//! Part of the port of claude-swap's `switcher.py` (0fe22a8): the refresh gate's rows of the
//! unclaimed stash (`stash.rs`), their format and their scan, kept beside the manifest they
//! live in. The gate parks a consumed generation's successor here when it can't persist it
//! (`stash_successor`), and its next pass looks for one before spending anything (the scan of
//! `_adopt_stashed_successor`, with `_retire_stash_entry`); writing an adopted successor back
//! into the slot stays the switcher's (`switcher/grant/stash.rs`).
//!
//! A row records `consumedFp`, the generation its successor superseded. When the slot still
//! stores exactly that generation, its refresh token is already spent and the stash holds the
//! live successor. The scan fails closed: a manifest that can't be read (or is corrupt while
//! entry files exist), or a matching row whose bytes can't be read this instant with no
//! readable sibling, is an error rather than "nothing stashed", since the gate would then
//! spend a generation a stashed successor may already have superseded (at one strike, that
//! POST quarantines a live account while its successor sits on disk). Rows no pass can ever
//! adopt are retired as they're met: a CAS conflict's (the store only moves forward), and a
//! byte-less one's. A row whose bytes are merely unreadable is kept: dropping it is the
//! person's call.

use serde_json::{Map, Value};

use crate::credentials::{CredentialStore, ManifestVerdict};
use crate::oauth::credential_fingerprint;
use crate::{AccountsError, Result};

const REASON: &str = "reason";
const CONFIG_SLOT: &str = "configSlot";
const CONSUMED_FP: &str = "consumedFp";
const FINGERPRINT: &str = "fingerprint";

/// Why the gate parked a successor, the row's `reason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StashReason {
    /// The backup write after the POST failed.
    PersistFailed,
    /// Koyo's `.lock` stayed held after the POST.
    PersistLockFailed,
    /// The backup couldn't be read after the POST (the Keychain locked during it), so the
    /// compare-and-swap couldn't be evaluated.
    StoreUnreadable,
    /// The backup was emptied during the POST: a removal, which a write-back would undo.
    SlotRemoved,
    /// A writer replaced the lineage during the POST; its newer one is adopted instead.
    CasConflict,
}

const REASONS: [StashReason; 5] = [
    StashReason::PersistFailed,
    StashReason::PersistLockFailed,
    StashReason::StoreUnreadable,
    StashReason::SlotRemoved,
    StashReason::CasConflict,
];

impl StashReason {
    /// cswap's reason string, as the manifest stores it.
    pub fn as_str(self) -> &'static str {
        match self {
            StashReason::PersistFailed => "consume-gate-persist-failed",
            StashReason::PersistLockFailed => "consume-gate-persist-lock-failed",
            StashReason::StoreUnreadable => "consume-gate-store-unreadable",
            StashReason::SlotRemoved => "consume-gate-slot-removed",
            StashReason::CasConflict => "consume-gate-cas-conflict",
        }
    }

    /// `_DEMOTING_STASH_REASONS`: the slot is left on the generation whose grant was just
    /// spent, so the gate reports `transient`, never success (activating it would install a
    /// token that can never refresh). A removed slot has nothing to activate, and a CAS
    /// conflict adopted a racing writer's newer lineage, which is what the caller asked for:
    /// neither demotes.
    pub fn demotes(self) -> bool {
        matches!(
            self,
            StashReason::PersistFailed
                | StashReason::PersistLockFailed
                | StashReason::StoreUnreadable
        )
    }

    /// The warning the gate logs when it parks a successor for this reason, cswap's notes.
    pub fn note(self, account_num: &str) -> String {
        match self {
            StashReason::StoreUnreadable => format!(
                "Account {account_num}'s stored credential was unreadable (keychain) after a \
                 refresh POST; successor stashed, nothing rewritten."
            ),
            StashReason::SlotRemoved => format!(
                "Account {account_num}'s stored credential disappeared during a refresh POST; \
                 successor stashed, nothing rewritten."
            ),
            StashReason::CasConflict => format!(
                "Backup lineage for account {account_num} moved during a refresh POST; successor \
                 stashed, adopting the newer store credential."
            ),
            StashReason::PersistLockFailed => format!(
                "Slot lock unavailable after consuming account {account_num}'s grant; successor \
                 stashed for the next pass."
            ),
            StashReason::PersistFailed => format!(
                "Persist failed after consuming account {account_num}'s grant; successor stashed \
                 for the next pass."
            ),
        }
    }

    fn parse(reason: &str) -> Option<StashReason> {
        REASONS.into_iter().find(|known| known.as_str() == reason)
    }
}

/// A row of the gate's, as it writes one: the manifest also holds the switch's safety copies
/// (`switcher/switch/rescue.rs`), whose rows carry no `consumedFp`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashMeta {
    pub reason: StashReason,
    /// The slot whose grant was spent, `configSlot`.
    pub config_slot: String,
    /// The generation the successor superseded, `consumedFp`: the adoption key.
    pub consumed_fp: Option<String>,
    /// The successor's own lineage, `fingerprint`.
    pub fingerprint: Option<String>,
}

impl StashMeta {
    /// The row of `successor`, parked for `reason` after slot `config_slot`'s grant of the
    /// generation `consumed_fp` was spent.
    pub fn successor(
        reason: StashReason,
        config_slot: &str,
        consumed_fp: Option<&str>,
        successor: &str,
    ) -> StashMeta {
        StashMeta {
            reason,
            config_slot: config_slot.to_owned(),
            consumed_fp: consumed_fp.map(str::to_owned),
            fingerprint: credential_fingerprint(successor),
        }
    }

    fn row(&self) -> Map<String, Value> {
        let text = |value: &Option<String>| value.clone().map_or(Value::Null, Value::from);
        let mut row = Map::new();
        row.insert(REASON.to_owned(), Value::from(self.reason.as_str()));
        row.insert(
            CONFIG_SLOT.to_owned(),
            Value::from(self.config_slot.clone()),
        );
        row.insert(CONSUMED_FP.to_owned(), text(&self.consumed_fp));
        row.insert(FINGERPRINT.to_owned(), text(&self.fingerprint));
        row
    }
}

/// A stashed successor of the slot's current generation, found by
/// [`CredentialStore::stashed_successor`]. No `Debug`: it holds a credential.
pub struct StashedSuccessor {
    pub entry_id: String,
    pub credentials: String,
    /// Why it was parked; `None` for a reason the gate doesn't write.
    pub reason: Option<StashReason>,
}

fn field<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

impl CredentialStore {
    /// cswap's `stash_successor`, its write: parks `successor` with `meta` as its row, the
    /// entry id returned.
    ///
    /// # Errors
    /// As [`Self::write_unclaimed_credential`].
    pub fn stash_successor(&self, successor: &str, meta: &StashMeta) -> Result<String> {
        self.write_unclaimed_credential(successor, meta.row())
    }

    /// `_retire_stash_entry`: drops one row as housekeeping, a failure logged. A raise after a
    /// completed adoption would read as a failed refresh and re-POST a generation the pass
    /// already spent; a stale row only costs a rescan.
    pub fn retire_stash_entry(&self, entry_id: &str, account_num: &str) {
        if let Err(error) = self.remove_unclaimed_credential(entry_id) {
            log::warn!(
                "Could not retire account {account_num}'s stash entry {entry_id}; leaving it for \
                 the next pass: {error}"
            );
        }
    }

    /// A row no pass can ever adopt, retired as the scan meets it.
    fn retire_dead(&self, entry_id: &str, account_num: &str, kind: &str, why: &str) {
        self.retire_stash_entry(entry_id, account_num);
        log::info!(
            "Retired account {account_num}'s {kind} stash entry: {why}, so no pass can ever \
             adopt it."
        );
    }

    /// The scan of `_adopt_stashed_successor`: a readable successor stashed for slot
    /// `account_num` whose `consumedFp` is the slot's current generation `consumed_fp`, or
    /// `None` when nothing applies; dead rows of the slot are retired on the way (the module
    /// says which). It isn't retired itself: the caller writes it back first.
    ///
    /// # Errors
    /// [`AccountsError::CredentialRead`] when the rows can't be established, or when the only
    /// matching rows are unreadable this instant.
    pub fn stashed_successor(
        &self,
        account_num: &str,
        consumed_fp: &str,
    ) -> Result<Option<StashedSuccessor>> {
        let (manifest, verdict) = self.read_stash_manifest_ex();
        if verdict == ManifestVerdict::Unreadable
            || (verdict == ManifestVerdict::Corrupt && self.stash_entry_files_exist())
        {
            // Not "nothing stashed": every row is the sole record of a spent generation, and
            // a corrupt manifest with no entry files falls through so the next manifest write
            // can set the bad file aside, its only repair.
            return Err(AccountsError::CredentialRead(format!(
                "the unclaimed manifest is {} and stashed entry files exist; deferring account \
                 {account_num}'s adoption rather than POSTing a generation a stashed successor \
                 may already have superseded",
                verdict.as_str()
            )));
        }
        // A row unreadable THIS instant must not stop the scan before a readable sibling on
        // the same generation is tried.
        let mut deferred_entry_id: Option<&str> = None;
        for (entry_id, row) in &manifest {
            if field(row, CONFIG_SLOT) != Some(account_num) {
                continue;
            }
            let reason = field(row, REASON).and_then(StashReason::parse);
            if field(row, CONSUMED_FP) != Some(consumed_fp) {
                if reason == Some(StashReason::CasConflict) {
                    // A conflict's row can never match: the store moved off the generation it
                    // superseded and only moves forward.
                    self.retire_dead(
                        entry_id,
                        account_num,
                        "CAS-conflict",
                        "its generation was superseded by the writer that won the race",
                    );
                } else if self.read_unclaimed_credential(entry_id) == (String::new(), false) {
                    // No bytes and no matching generation: what a failed retire leaves. An
                    // unreadable row reads `("", true)` and survives.
                    self.retire_dead(
                        entry_id,
                        account_num,
                        "byte-less",
                        "its credential is gone and its generation has passed",
                    );
                }
                continue;
            }
            let (credentials, unreadable) = self.read_unclaimed_credential(entry_id);
            if unreadable {
                deferred_entry_id.get_or_insert(entry_id);
                continue;
            }
            if credentials.is_empty() {
                // Absent or corrupt: the bytes are gone for good, so nothing can adopt it.
                self.retire_dead(
                    entry_id,
                    account_num,
                    "unreadable-bytes",
                    "its generation is gone",
                );
                continue;
            }
            return Ok(Some(StashedSuccessor {
                entry_id: entry_id.clone(),
                credentials,
                reason,
            }));
        }
        if let Some(entry_id) = deferred_entry_id {
            return Err(AccountsError::CredentialRead(format!(
                "stash entry {entry_id} for account {account_num} is unreadable; deferring \
                 adoption rather than discarding its generation"
            )));
        }
        Ok(None)
    }
}

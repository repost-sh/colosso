//! Port of claude-swap's `switcher.py` (0fe22a8), `_classify_outgoing_credential` with the
//! `_attribute_live_credential` it delegates to: what the switch-time backup may do with the
//! live credential.
//!
//! The global config says which slot is active; only the classification says who owns the live
//! bytes (cswap #117: an external write there used to destroy the outgoing slot's refresh
//! token). The identity oracle is advisory: an owner that can't be established falls back to
//! the exact pre-fix backup, so endpoint state never decides whether a switch completes.
//!
//! Left out: the `api-key-mismatch` verdict, which only an API-key slot reaches (Koyo has no
//! API-key accounts), so the classification is the attribution, under cswap's outer name.

use std::fmt;

use crate::Result;
use crate::oauth::value::truthy_at;
use crate::oauth::{AccountIdentity, credential_fingerprint, extract_oauth_data, has_login_token};
use crate::switcher::Switcher;
use crate::switcher::identity::same_lineage;
use crate::switcher::store::{Data, accounts, record, text};
use crate::switcher::switch::Outgoing;

/// The managed slot a foreign live credential belongs to, with its backup as the attribution
/// read it (`read_account_credentials_ex`), so the rescue doesn't read it again.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ForeignSlot {
    pub num: String,
    /// The slot's stored credential, `""` for none.
    pub backup: String,
    /// The read that gave `""` failed rather than found nothing.
    pub unreadable: bool,
}

impl fmt::Debug for ForeignSlot {
    /// The slot alone: the backup is a credential.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForeignSlot")
            .field("num", &self.num)
            .field("unreadable", &self.unreadable)
            .finish_non_exhaustive()
    }
}

/// The verdict on the outgoing slot's live credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OutgoingKind {
    /// Byte-identical to the slot's backup: nothing to capture.
    OwnBytes,
    /// The same refresh-token lineage (the access token rotated): backed up.
    OwnFamily,
    /// A full rotation the profile resolved to this slot's identity: backed up (the re-sync
    /// that keeps slots alive across Claude Code's routine rotations).
    OwnRotated,
    /// Uuid-positively another managed slot's, holding a different lineage: preserved in a
    /// safety copy, never written into any slot (identity proves ownership, not freshness).
    Foreign(ForeignSlot),
    /// Another managed slot's, whose backup already holds this lineage: nothing to do.
    ForeignSynced(String),
    /// An OAuth blob with its tokens emptied (Claude Code's `invalid_grant` reaction): never
    /// written into a slot, nothing worth preserving.
    Wiped,
    /// No `claudeAiOauth` at all while the slot's backup has one (an API key, or an item left
    /// holding only `mcpOAuth`, #383): never written into a slot.
    OauthAbsent,
    /// A structurally complete identity matching no managed slot: preserved.
    Alien,
    /// The switch-time oracle failed, but a probe in this process already condemned this
    /// lineage: preserved, never written.
    KnownForeign,
    /// Ownership couldn't be established (offline, endpoint failure, a partial answer, bytes
    /// moved since the pre-lock read): the pre-fix backup.
    Unresolved,
}

impl OutgoingKind {
    /// cswap's verdict string, the safety copy's `reason`.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            OutgoingKind::OwnBytes => "own-bytes",
            OutgoingKind::OwnFamily => "own-family",
            OutgoingKind::OwnRotated => "own-rotated",
            OutgoingKind::Foreign(_) => "foreign",
            OutgoingKind::ForeignSynced(_) => "foreign-synced",
            OutgoingKind::Wiped => "wiped",
            OutgoingKind::OauthAbsent => "oauth-absent",
            OutgoingKind::Alien => "alien",
            OutgoingKind::KnownForeign => "known-foreign",
            OutgoingKind::Unresolved => "unresolved",
        }
    }
}

impl Switcher {
    /// `_classify_outgoing_credential` (`_attribute_live_credential`): the verdict for
    /// `original_creds`, read live while `outgoing` is active, with its pre-lock provenance and
    /// the roster `data` read under the lock.
    ///
    /// # Errors
    /// As [`Switcher::lineage_verdict`].
    pub(crate) fn classify_outgoing_credential(
        &self,
        outgoing: &Outgoing<'_>,
        original_creds: &str,
        data: &Data,
    ) -> Result<OutgoingKind> {
        let backup = self
            .store
            .read_account_credentials(outgoing.account, outgoing.email, false);
        if !backup.is_empty() && backup == original_creds {
            return Ok(OutgoingKind::OwnBytes);
        }
        if !backup.is_empty()
            && credential_fingerprint(&backup) == credential_fingerprint(original_creds)
        {
            return Ok(OutgoingKind::OwnFamily);
        }
        let live_oauth = extract_oauth_data(original_creds);
        if live_oauth
            .as_ref()
            .is_some_and(|live| !has_login_token(live))
        {
            return Ok(OutgoingKind::Wiped);
        }
        if live_oauth.is_none()
            && !backup.is_empty()
            && extract_oauth_data(&backup).is_some_and(|oauth| !oauth.is_empty())
        {
            // Only the asymmetry, nothing to preserve live and a refresh token to lose in the
            // slot, is the destruction case.
            return Ok(OutgoingKind::OauthAbsent);
        }
        let provenance = outgoing.provenance;
        let resolved = match &provenance.resolved {
            Some(resolved) if provenance.live.as_deref() == Some(original_creds) => resolved,
            _ => {
                let verdict =
                    self.lineage_verdict(outgoing.account, outgoing.email, original_creds)?;
                if verdict == Some(false) {
                    return Ok(OutgoingKind::KnownForeign);
                }
                return Ok(OutgoingKind::Unresolved);
            }
        };
        Ok(self.attribute_resolved(outgoing.account, original_creds, resolved, data))
    }

    /// The attribution's second half, once the live bytes' owner was resolved.
    fn attribute_resolved(
        &self,
        current_account: &str,
        original_creds: &str,
        resolved: &AccountIdentity,
        data: &Data,
    ) -> OutgoingKind {
        let r_email = resolved.email.as_deref().unwrap_or_default();
        let r_org = resolved.organization_uuid.as_deref().unwrap_or_default();
        let r_uuid = resolved.uuid.trim();
        // The outgoing slot's uuid first: robust to a partial answer (a drifted schema may drop
        // the email or the org) and to an email that changed. The org must agree only when both
        // sides record one.
        let own = record(data, current_account);
        let own_uuid = text(own, "uuid").trim();
        let own_org = text(own, "organizationUuid");
        if !r_uuid.is_empty()
            && !own_uuid.is_empty()
            && r_uuid == own_uuid
            && (r_org.is_empty() || own_org.is_empty() || r_org == own_org)
        {
            return OutgoingKind::OwnRotated;
        }
        let stored_uuid = |slot: &str| text(record(data, slot), "uuid").trim().to_owned();
        let mut slot = if r_email.is_empty() {
            None
        } else {
            Switcher::find_account_slot(data, r_email, r_org)
        };
        if let Some(found) = &slot
            && !r_uuid.is_empty()
        {
            // Both sides carrying a uuid must agree: an email and org match under another uuid
            // is a different account wearing a recycled email.
            let stored = stored_uuid(found);
            if !stored.is_empty() && stored != r_uuid {
                slot = None;
            }
        }
        if slot.is_none() && !r_uuid.is_empty() {
            // The account uuid (org-scoped), for a slot whose stored email is stale.
            slot = accounts(data)
                .into_iter()
                .flatten()
                .find_map(|(num, account)| {
                    let account = account.as_object()?;
                    let uuid = account.get("uuid").and_then(serde_json::Value::as_str);
                    (truthy_at(account.get("uuid"))
                        && uuid == Some(r_uuid)
                        && text(Some(account), "organizationUuid") == r_org)
                        .then(|| num.clone())
                });
        }
        let Some(slot) = slot else {
            // A positive alien needs an email and an organisation; anything partial is
            // indistinguishable from schema drift and fails open.
            if !r_email.is_empty() && resolved.organization_uuid.is_some() {
                return OutgoingKind::Alien;
            }
            return OutgoingKind::Unresolved;
        };
        if slot == current_account {
            return OutgoingKind::OwnRotated;
        }
        // A cross-slot attribution must be uuid-positive to name that slot.
        if r_uuid.is_empty() || stored_uuid(&slot) != r_uuid {
            return OutgoingKind::Alien;
        }
        let foreign_email = text(record(data, &slot), "email");
        let (backup, unreadable) =
            self.store
                .read_account_credentials_ex(&slot, foreign_email, false);
        if !backup.is_empty() && same_lineage(&backup, original_creds) {
            return OutgoingKind::ForeignSynced(slot);
        }
        OutgoingKind::Foreign(ForeignSlot {
            num: slot,
            backup,
            unreadable,
        })
    }
}

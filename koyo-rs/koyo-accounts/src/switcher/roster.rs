//! The managed slots as one value, over claude-swap's `switcher.py` (0fe22a8) roster reads:
//! one read of `sequence.json` and one of the live identity (`_get_current_identity_triple`),
//! which the accounts info, the snapshot and the subscription plans share. cswap reads the
//! roster again in each (`accounts_snapshot` twice, `_collect_subscription_plans` a third
//! time), so one pass could see two rosters; here a pass sees one.
//!
//! The active slot is the live identity's (`_find_account_slot`), never the recorded
//! `activeAccountNumber`: an unmanaged login must not be taken for a slot. Each slot's
//! `has_backup` is `_account_is_switchable` on the same read, through the display cache. A slot
//! the sequence names without a record reads as `_build_accounts_info` reads one: email
//! `unknown`, nothing else.
//!
//! Left out: the alias, with aliases, and `_get_sequence_data_migrated` (Koyo's store starts
//! new, so the roster is read as it is).

use serde_json::Value;

use crate::Result;
use crate::switcher::store::{record, sequence, text};
use crate::switcher::{SlotIdentity, Switcher};

/// The managed slots and the Mac's login, from one read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roster {
    /// The slot holding the Mac's login, cswap's `current_account_number`; `None` when there is
    /// no login or it isn't managed.
    pub current: Option<String>,
    /// The Mac's login, managed or not; `live.is_some()` is cswap's `has_live_login`.
    pub live: Option<SlotIdentity>,
    /// Every slot of the sequence, in its order.
    pub slots: Vec<RosterSlot>,
}

/// One managed slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterSlot {
    pub number: String,
    /// The stored identity: the email, the organization and the account uuid (trimmed).
    pub identity: SlotIdentity,
    /// The stored `organizationName`, `""` for a personal account.
    pub organization_name: String,
    /// cswap's `_account_is_switchable`: a stored credential and a config backup, so the slot
    /// can be activated without adding it again.
    pub has_backup: bool,
    /// Held out of auto-switch (`policy.rs`); still a valid explicit switch target.
    pub disabled: bool,
}

impl RosterSlot {
    /// Whether auto-switch may pick it, cswap's `switchable_account_numbers` membership.
    pub fn switchable(&self) -> bool {
        self.has_backup && !self.disabled
    }
}

/// The engine's lookups on the roster.
impl Roster {
    pub(crate) fn slot(&self, number: &str) -> Option<&RosterSlot> {
        self.slots.iter().find(|slot| slot.number == number)
    }

    /// cswap's `account_email`: the slot's stored email, empty when unknown.
    pub(crate) fn email(&self, number: &str) -> &str {
        self.slot(number)
            .map_or("", |slot| slot.identity.email.as_str())
    }
}

impl Switcher {
    /// The roster as it is now (the module says what is read).
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`] and [`Switcher::account_is_switchable`].
    pub fn roster(&self) -> Result<Roster> {
        let data = self.sequence_data_or_empty()?;
        let live = self.get_current_identity_triple();
        let current = Switcher::live_slot(&data, live.as_ref());
        let mut slots = Vec::new();
        for number in sequence(&data) {
            let account = record(&data, &number);
            let email = account
                .and_then(|account| account.get("email"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            slots.push(RosterSlot {
                identity: SlotIdentity {
                    email: email.to_owned(),
                    organization_uuid: text(account, "organizationUuid").to_owned(),
                    uuid: text(account, "uuid").trim().to_owned(),
                },
                organization_name: text(account, "organizationName").to_owned(),
                has_backup: self.account_is_switchable(&data, &number)?,
                disabled: Switcher::disabled_from_data(&data, &number),
                number,
            });
        }
        Ok(Roster {
            current,
            live,
            slots,
        })
    }
}

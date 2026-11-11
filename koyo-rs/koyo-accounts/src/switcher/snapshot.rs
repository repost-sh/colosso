//! Port of claude-swap's `switcher.py` (0fe22a8) `accounts_snapshot`, with `models.py`'s
//! `AccountSnapshot` and `AccountsSnapshot`: every managed account as one coherent view.
//! Identity, the active slot, eligibility and usage come from one roster read, one
//! accounts-info build and one collection pass, so the view can't disagree with itself about
//! the active slot or freshness, as two calls interleaved with another collector could. cswap
//! reads the roster again for `switchable` and `disabled` after the pass; here they're the
//! roster's.
//!
//! The subscription plan is shown as [`Plan`], its label read from the stored record once here
//! (`subscription::plan_label`).
//!
//! Left out of the row: `alias` (aliases aren't ported), `kind` (Koyo manages OAuth accounts
//! only, so it would always be `oauth`) and `policy` (it carried per-account thresholds and
//! priority tiers beside `disabled`; only `disabled` is ported, and it's on the slot).

use serde_json::Value;

use crate::Result;
use crate::subscription::plan_label;
use crate::switcher::{Fetch, Roster, RosterSlot, SlotIdentity, Switcher};
use crate::usage_store::UsageEntry;

/// An account's subscription plan as the app shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The label (`subscription::plan_label`): "Max 20x", "Team", …; `None` for a plan with
    /// none to give.
    pub label: Option<String>,
}

impl Plan {
    /// The plan of a stored record (`subscription::valid_record`'s shape).
    fn of(record: &Value) -> Plan {
        Plan {
            label: plan_label(record.get("plan").and_then(Value::as_object)),
        }
    }
}

/// One managed account as the app shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct AccountSnapshot {
    /// The roster's slot: its number, identity, organization name, `has_backup` and
    /// `disabled`.
    pub slot: RosterSlot,
    pub is_active: bool,
    /// The store-backed usage: views read `last_good` and `age_s` (old data shown with its
    /// age); `sentinel` replaces the bars ("token expired", "keychain unavailable", …).
    pub usage: UsageEntry,
    /// The subscription plan, `None` for a slot whose plan was never read.
    pub plan: Option<Plan>,
}

/// Every managed account in sequence order, from one pass.
#[derive(Clone, Debug, PartialEq)]
pub struct AccountsSnapshot {
    /// The slot holding the Mac's login ([`Roster::current`]).
    pub current: Option<String>,
    /// The Mac's login, managed or not ([`Roster::live`]).
    pub live: Option<SlotIdentity>,
    pub accounts: Vec<AccountSnapshot>,
}

impl Switcher {
    /// cswap's `accounts_snapshot`. `fetch` has the collection's meaning ([`Fetch`]); one that
    /// fetches nothing reads the plans from their file only.
    ///
    /// # Errors
    /// As [`Switcher::roster`], and the usage store's lock or write.
    pub fn accounts_snapshot(&self, fetch: Fetch<'_>) -> Result<AccountsSnapshot> {
        let Roster {
            current,
            live,
            slots,
        } = self.roster()?;
        let infos = self.accounts_info(current.as_deref(), slots);
        let mut entries = self.collect_usage_entries(&infos, fetch)?;
        let plans = self.plans_for_pass(&infos, fetch)?;
        let accounts = infos
            .into_iter()
            .map(|info| AccountSnapshot {
                is_active: info.is_active(),
                usage: entries.remove(info.num()).unwrap_or_default(),
                plan: plans.get(info.num()).map(Plan::of),
                slot: info.slot,
            })
            .collect();
        Ok(AccountsSnapshot {
            current,
            live,
            accounts,
        })
    }
}

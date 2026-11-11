//! Koyo's own addition, not in claude-swap: each chat reply's `rate_limit_event` updates the
//! active account's reading for free (`docs/claude-accounts.md` 4). The agent host passes the
//! windows Claude Code read from the response's headers ([`LiveLimits`]); they are made a
//! reading as cswap makes one from those headers (`oauth/headers.rs`).
//!
//! The reading is laid over the active slot's stored last good one, so a window the response
//! didn't carry keeps its stored value (a reply without the `7d_oi` bucket doesn't erase it,
//! and `spend` stays), and adopted as a reading of now, in one locked step of the usage store
//! (`UsageStore::adopt_over`): it never replaces a newer stored one, and leaves the fetch
//! state alone. No network and no credential read: the active slot is found from the live
//! config's identity.

use crate::Result;
use crate::switcher::{LiveLimits, Switcher};
use crate::usage_store::UsageEntry;

impl Switcher {
    /// Takes a chat reply's rate-limit windows as the active account's reading (the module
    /// says how): the active slot's entry as adopted, for the app to redraw it, or `None` when
    /// nothing was: a reading with no window or an unusable one, no managed active login, or a
    /// stored reading at least as new.
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`], and the usage store's lock or write.
    pub fn adopt_live_limits(&self, limits: LiveLimits) -> Result<Option<UsageEntry>> {
        let Some(reading) = limits.reading(self.env.now()) else {
            return Ok(None);
        };
        let Some(live) = self.get_current_identity_triple() else {
            return Ok(None);
        };
        let data = self.sequence_data_or_empty()?;
        let Some(num) = Switcher::live_slot(&data, Some(&live)) else {
            return Ok(None);
        };
        let inputs = self.poll_policy_inputs();
        self.usage_store.adopt_over(
            &num,
            &(live.email, live.organization_uuid),
            &reading,
            &inputs.model_names(),
        )
    }
}

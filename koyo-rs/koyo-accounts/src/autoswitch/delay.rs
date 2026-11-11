//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): how long to wait before the
//! next tick (`_next_delay`, `_respect_poll_plan`), which each tick reports. The loop itself is
//! the app's: it waits that long.
//!
//! cswap re-reads the active account and its row after every tick; here the tick keeps the
//! active row's plan from its own collection, and re-reads only after a switch, whose
//! `switch_to` replanned the new active row.

use std::collections::BTreeSet;

use crate::poll_policy::{URGENT_INTERVAL_S, jittered};

use super::{ActivePlan, AutoSwitchEngine, MAX_SLEEP_S, NO_RESET_FALLBACK_S, Wait};

impl AutoSwitchEngine {
    /// The wait after a tick, in seconds, as the tick left it in `wait`. cswap's `_next_delay`
    /// also asks the outcome, but each wait is set only on the outcome cswap pairs it with (a
    /// reset or a long wait on a blocked tick, the idle hold's on a tick that holds) and a failed
    /// tick resets it, so the wait alone decides.
    pub(crate) fn next_delay(&self) -> f64 {
        let interval = self.settings.interval_seconds;
        match self.wait {
            Wait::Until(until) => (until - self.now()).max(interval).min(MAX_SLEEP_S),
            // Truly exhausted with no reset known, or no candidates; or idle-holding: Claude
            // Code is idle on an expired token and nothing changes until the person is back, so
            // crawl, and protection resumes one slow tick after they are.
            Wait::Long => interval.max(NO_RESET_FALLBACK_S),
            // Blocked on what can resolve any tick (hysteresis, unreadable usage), or not
            // blocked: the normal cadence, so the at-limit escape isn't missed, with ±10%
            // jitter, so several machines don't synchronize their requests.
            Wait::Interval => self.respect_poll_plan(jittered(interval, (self.rng)())),
        }
    }

    /// Shortens a normal-cadence wait to the active row's own next poll.
    ///
    /// The planner tightens the active row to `URGENT_INTERVAL_S` while it burns toward the
    /// line, but a loop waiting `interval_seconds` ran the plan late (measured mid-episode: the
    /// row was due 112 s ago with minutes of sleep left). Only ever shortens, never below the
    /// planner's floor, which holds the 429 budget: the deadline is clamped, not the result, so
    /// a wait already below the floor stays as it is. Best-effort: the unshortened wait is safe.
    fn respect_poll_plan(&self, delay: f64) -> f64 {
        let next_poll_at = match self.active_plan {
            ActivePlan::Unknown => None,
            ActivePlan::Kept(next_poll_at) => Some(next_poll_at),
            ActivePlan::Replanned => self.replanned_next_poll(),
        };
        let Some(next_poll_at) = next_poll_at else {
            return delay;
        };
        delay.min((next_poll_at - self.now()).max(URGENT_INTERVAL_S))
    }

    /// The new active account's next poll after a switch, `None` when it can't be read.
    fn replanned_next_poll(&self) -> Option<f64> {
        let current = self.host.roster().ok()?.current?;
        self.host
            .usage_entries_by_account(&BTreeSet::new(), false)
            .ok()?
            .get(&current)?
            .next_poll_at
    }
}

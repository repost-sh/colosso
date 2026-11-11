//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): which accounts are read on which
//! tick (`_collect_scheduled_usage`).
//!
//! The baseline is O(1) requests a tick: the active account when its persisted poll plan says
//! it's due, plus one due candidate, the one with the stalest data; everyone else is served from
//! the usage store. The per-account cadence (movement, distance to the line, urgent mode, 429
//! recovery) lives in `poll_policy`, is stored with each row by whichever collector fetched, and
//! is shared by every surface. The engine escalates to a refresh of every candidate only when a
//! switch could be near: a window of the active account within `ESCALATION_MARGIN_PCT` of the
//! line (`poll_policy::near_line`), or the active's usage unknown (failover must not run on stale
//! candidate data). Stalest-first needs no cursor: it reads the persisted store. The collector
//! enforces backoff even for the active account (a `Retry-After` is never defeated), and during
//! an idle hold no candidate is polled.

use std::collections::{BTreeMap, BTreeSet};

use crate::Result;
use crate::oauth::{USAGE_TOKEN_EXPIRED, account_headroom};
use crate::poll_policy::{
    ACTIVE_MAX_INTERVAL_S, EXHAUSTED_INTERVAL_S, MIN_INTERVAL_S, binding_pct, near_line,
};
use crate::usage_store::{Decision, UsageEntry, due_candidate, plan_oversleeps_interval};

use super::windows::Reading;
use super::{AutoSwitchEngine, MODELS};

impl AutoSwitchEngine {
    /// Two-phase collection with an O(1) baseline, on the tick's line, so one tick fetches and
    /// decides on the same values. `candidates` and `disabled` are the switchable and disabled
    /// slots but the active and the quarantined: a quarantined account can't be a target, so
    /// the one alternate slot (or an escalation fetch) is never spent on one.
    pub(crate) fn collect_scheduled_usage(
        &self,
        current: &str,
        candidates: &[String],
        disabled: &[String],
        threshold: f64,
    ) -> Result<BTreeMap<String, UsageEntry>> {
        let now = self.now();
        let pre = self
            .host
            .usage_entries_by_account(&BTreeSet::new(), false)?;
        let mut plan: BTreeSet<String> = BTreeSet::new();
        let active_pre = pre.get(current);
        // The active account is nominated when never fetched, poll-due per its plan, or (no
        // plan yet) past the cadence floor. A candidate-style plan (slower than any active plan)
        // left from a role change the switcher never saw, a manual login, is overridden past the
        // active age cap; an exhausted account carries its own bounded plan.
        let stale_candidate_plan = active_pre.is_some_and(|entry| {
            entry.age_s.is_some_and(|age| age >= ACTIVE_MAX_INTERVAL_S)
                && entry.poll_interval_s.unwrap_or(0.) > ACTIVE_MAX_INTERVAL_S
                && binding_pct(entry.last_good.as_ref(), MODELS).unwrap_or(0.) < 100.
        });
        let overslept_plan = active_pre.is_some_and(|entry| plan_oversleeps_interval(entry, now));
        if active_poll_due(active_pre, now) || stale_candidate_plan || overslept_plan {
            plan.insert(current.to_owned());
        }
        if self.idle_hold_since.is_none()
            && let Some(pick) =
                due_candidate(candidates, &pre, now).or_else(|| due_candidate(disabled, &pre, now))
        {
            plan.insert(pick);
        }
        // A candidate-style plan on the active slot is overridden after the age cap; every
        // other nomination keeps a valid future plan under the store's lock. With nothing
        // nominated that pass would fetch nothing: it would be the read above again.
        let entries = if plan.is_empty() {
            pre
        } else {
            self.host
                .usage_entries_by_account(&plan, !stale_candidate_plan)?
        };

        let active = entries
            .get(current)
            .map(|entry| Reading::of(entry, threshold));
        // Any window of the active near the line: a 5h window closing on it under a flat,
        // higher weekly window is a switch that is near, whatever the binding max says. An
        // active with no reading escalates, but not one whose token expired on an idle machine.
        let escalate = !candidates.is_empty()
            && active.as_ref().is_none_or(|active| match active.headroom {
                Some(_) => active
                    .windows
                    .iter()
                    .any(|window| near_line(window, threshold, None)),
                None => !active.is_sentinel(USAGE_TOKEN_EXPIRED),
            });
        if !escalate {
            return Ok(entries);
        }
        let mut fetch: BTreeSet<String> = candidates.iter().cloned().collect();
        fetch.insert(current.to_owned());
        // Escalation may beat ordinary candidate plans for a fresh decision, but a
        // decision-trusted exhausted row can't be a target: its wider post-429 plan is kept.
        fetch.retain(|num| {
            entries
                .get(num)
                .is_none_or(|entry| !keeps_exhausted_plan(entry, now))
        });
        self.host.usage_entries_by_account(&fetch, false)
    }
}

/// Whether the active account's row is due for a fetch: never fetched, past its `nextPollAt`, or
/// (no plan yet) older than the cadence floor ([`MIN_INTERVAL_S`]).
fn active_poll_due(entry: Option<&UsageEntry>, now: f64) -> bool {
    let Some(entry) = entry else {
        return true;
    };
    match (entry.age_s, entry.next_poll_at) {
        (None, _) => true,
        (_, Some(next_poll_at)) => now >= next_poll_at,
        (Some(age), None) => age >= MIN_INTERVAL_S,
    }
}

/// Whether an escalation keeps a row's plan rather than refetching it: a decision-trusted
/// exhausted row can't be a target, so a future plan wider than the exhausted cadence (learned
/// after 429s) stands instead of refetching that token at the all-exhausted wake cadence.
fn keeps_exhausted_plan(entry: &UsageEntry, now: f64) -> bool {
    let headroom = account_headroom(entry.decision_value().and_then(Decision::usage), MODELS);
    entry.next_poll_at.is_some_and(|at| now < at)
        && entry.poll_interval_s.unwrap_or(0.) > EXHAUSTED_INTERVAL_S
        && headroom.is_some_and(|headroom| headroom <= 0.)
}

//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): one evaluation (`tick`,
//! `_tick_inner`), from the state file, the roster and the poll to the trigger, the candidates
//! and the blocked outcomes; the switch itself is `perform.rs`.
//!
//! cswap resolves an effective threshold per account each tick (`_resolve_thresholds`) and
//! re-pins the collector's cadence to the fleet minimum every tick (`_min_effective_threshold`,
//! `_refresh_poll_policy_inputs`), since another process may have changed a stored line; with
//! one global threshold, set only through this engine, the line is the tick's
//! `settings.threshold` and the pin moves with [`AutoSwitchEngine::set_threshold`]. The
//! API-key, consume-first and priority branches are left out with their accounts and
//! strategies.

use std::collections::BTreeMap;
use std::mem;

use crate::Result;
use crate::oauth::USAGE_TOKEN_EXPIRED;
use crate::poll_policy::RESET_SLACK_S;
use crate::switcher::{AccountRef, Roster, RosterSlot};
use crate::usage_store::UsageEntry;

use super::events::{
    AllExhaustedEvent, AutoSwitchEvent, ErrorEvent, NoSwitchEvent, NoSwitchReason, PollEvent,
    TickOutcome, TickReport, Trigger, below_line_detail,
};
use super::perform::Departure;
use super::rank::RankInput;
use super::release::rank_with_bar;
use super::state::AutoSwitchState;
use super::windows::{Reading, Readings, earliest_recovery, readings};
use super::{ActivePlan, AutoSwitchEngine, IDLE_HOLD_MAX_S, Wait};

impl AutoSwitchEngine {
    /// Evaluates once: polls usage, maybe switches, and says how long to wait before the next
    /// tick. Never fails: an error is an [`ErrorEvent`] and [`TickOutcome::Error`].
    pub fn tick(&mut self) -> TickReport {
        self.wait = Wait::Interval;
        self.active_plan = ActivePlan::Unknown;
        let mut state = self.read_state();
        let result = self
            .tick_inner(&mut state)
            .and_then(|outcome| self.persist_health_counters(&state).map(|()| outcome));
        let outcome = result.unwrap_or_else(|error| {
            // A failed tick waits the normal cadence, whatever it had decided before failing.
            self.wait = Wait::Interval;
            self.emit_error(error.to_string());
            TickOutcome::Error
        });
        TickReport {
            outcome,
            delay: self.next_delay(),
            events: mem::take(&mut self.events),
        }
    }

    pub(crate) fn emit_error(&mut self, message: String) {
        self.emit(AutoSwitchEvent::Error(ErrorEvent {
            ts: self.now(),
            message,
        }));
    }

    pub(crate) fn no_switch(&mut self, reason: NoSwitchReason, detail: &str) {
        self.emit(AutoSwitchEvent::NoSwitch(NoSwitchEvent {
            ts: self.now(),
            reason,
            detail: detail.to_owned(),
        }));
    }

    fn tick_inner(&mut self, state: &mut AutoSwitchState) -> Result<TickOutcome> {
        let settings = self.settings.clone();
        let roster = self.host.roster()?;
        self.release_recovered_quarantines(&roster, state)?;

        // A tick with no managed active account keeps the counting context; another account
        // reseeds from the state file, so its counts are never inherited.
        if let Some(current) = &roster.current
            && self.health_account.as_ref() != Some(current)
        {
            self.load_health_counters(state, current);
            self.health_account = Some(current.clone());
        }
        let Some(current) = roster.current.as_deref() else {
            return Ok(self.no_active_account(&roster));
        };

        // Every candidate is an OAuth account; the no-return bar is applied where the choice
        // is made (`release.rs`), not in this census of what exists.
        let census = |wanted: fn(&RosterSlot) -> bool| -> Vec<String> {
            roster
                .slots
                .iter()
                .filter(|slot| {
                    wanted(slot)
                        && slot.number != current
                        && !state.quarantine.contains_key(&slot.number)
                })
                .map(|slot| slot.number.clone())
                .collect()
        };
        let candidates = census(RosterSlot::switchable);
        let disabled = census(|slot| slot.disabled);
        let entries =
            self.collect_scheduled_usage(current, &candidates, &disabled, settings.threshold)?;
        if let Some(next_poll_at) = entries.get(current).and_then(|entry| entry.next_poll_at) {
            self.active_plan = ActivePlan::Kept(next_poll_at);
        }
        let readings = readings(&entries, settings.threshold);
        self.emit_poll(&roster, current, &entries, &readings);

        let active = readings.get(current);
        let active_headroom = active.and_then(|active| active.headroom);
        let Some(trigger) = self.trigger_for(active) else {
            return Ok(TickOutcome::NoAction);
        };
        if trigger == Trigger::Proactive && state.in_cooldown(self.now(), settings.cooldown_seconds)
        {
            self.no_switch(NoSwitchReason::Cooldown, "");
            return Ok(TickOutcome::NoAction);
        }
        if candidates.is_empty() {
            // Won't change until an account is added or recovered: no point polling at the
            // full cadence.
            self.wait = Wait::Long;
            self.no_switch(NoSwitchReason::NoCandidates, "");
            return Ok(TickOutcome::Blocked);
        }

        let input = RankInput {
            settings: &settings,
            trigger,
            candidates: &candidates,
            readings: &readings,
            current,
            active_headroom,
            now: self.now(),
        };
        let (ordered, any_known) = rank_with_bar(state, &input);
        if ordered.is_empty() {
            return Ok(self.report_blocked(&input, any_known));
        }
        // The departure snapshot of the account being left, from the same readings and instant
        // the ranking decided on.
        let left = Departure {
            headroom: active_headroom,
            recovery: active.map_or(f64::INFINITY, |active| active.reset_key(input.now)),
        };
        self.switch_to_first(&roster, &ordered, trigger, left, state)
    }

    /// What moves the engine this tick, or `None` when the tick holds (it has said why): the
    /// active account over the line (proactive) or at its limit, or unreadable for
    /// `unhealthy_ticks` ticks (failover), an expired token on an idle machine holding instead.
    fn trigger_for(&mut self, active: Option<&Reading<'_>>) -> Option<Trigger> {
        // A headroom and a binding window come from the same windows: both or neither.
        if let Some(active) = active
            && let (Some(active_headroom), Some(binding)) = (active.headroom, &active.binding)
        {
            self.unhealthy_ticks = 0;
            self.idle_hold_since = None;
            if !binding.over() {
                let detail = below_line_detail(binding);
                self.no_switch(NoSwitchReason::BelowThreshold, &detail);
                return None;
            }
            // Exhaustion is physical: at-limit reads the raw headroom.
            return Some(if active_headroom <= 0. {
                Trigger::AtLimit
            } else {
                Trigger::Proactive
            });
        }
        if active.is_some_and(|active| active.is_sentinel(USAGE_TOKEN_EXPIRED)) {
            // Expired, and the refresh couldn't complete this pass (lock contention, an
            // unattributable lineage, a failed persist, or the row's backoff). Later passes
            // retry: no quota burns, nothing to switch for yet, so crawl slowly instead of
            // spending failover ticks.
            let now = self.now();
            let since = *self.idle_hold_since.get_or_insert(now);
            if now - since <= IDLE_HOLD_MAX_S {
                self.unhealthy_ticks = 0;
                self.wait = Wait::Long;
                self.no_switch(
                    NoSwitchReason::ActiveIdle,
                    "token expired while Claude Code is idle; resumes on next use",
                );
                return None;
            }
            // Held far longer than an idle nap needs: likely a dead refresh token with an
            // active user, so count as unhealthy and fail over after all.
            log::warn!(
                "Active token expired and owned for over {:.0} minutes; resuming unhealthy \
                 counting (dead refresh token?)",
                IDLE_HOLD_MAX_S / 60.
            );
        } else {
            self.idle_hold_since = None;
        }
        self.unhealthy_ticks += 1;
        let needed = self.settings.unhealthy_ticks;
        if self.unhealthy_ticks < needed {
            let detail = format!("{}/{needed} before failover", self.unhealthy_ticks);
            self.no_switch(NoSwitchReason::ActiveUsageUnknown, &detail);
            return None;
        }
        Some(Trigger::Failover)
    }

    /// No managed active account: report it and hold.
    fn no_active_account(&mut self, roster: &Roster) -> TickOutcome {
        self.emit(AutoSwitchEvent::Poll(PollEvent {
            ts: self.now(),
            active: None,
            headroom: BTreeMap::new(),
            threshold: self.settings.threshold,
            fetch_errors: BTreeMap::new(),
            windows: BTreeMap::new(),
            binding: None,
            emails: BTreeMap::new(),
        }));
        if roster.live.is_some() {
            // A live login Koyo doesn't manage: never act, a switch would overwrite it
            // without a backup.
            self.no_switch(
                NoSwitchReason::UnmanagedActiveAccount,
                "Add Current Login to include it in rotation",
            );
        } else {
            self.no_switch(
                NoSwitchReason::NoActiveAccount,
                "sign in with Claude Code and Add Current Login first",
            );
        }
        TickOutcome::NoAction
    }

    fn emit_poll(
        &mut self,
        roster: &Roster,
        current: &str,
        entries: &BTreeMap<String, UsageEntry>,
        readings: &Readings<'_>,
    ) {
        let fetch_errors = entries
            .iter()
            .filter(|(num, _)| {
                readings
                    .get(*num)
                    .is_none_or(|reading| reading.decision.is_none())
            })
            .filter_map(|(num, entry)| {
                entry
                    .last_error
                    .clone()
                    .filter(|error| !error.is_empty())
                    .map(|error| (num.clone(), error))
            })
            .collect();
        let windows = readings
            .iter()
            .map(|(num, reading)| (num.clone(), reading.window_pcts()))
            .filter(|(_, pcts)| !pcts.is_empty())
            .collect();
        self.emit(AutoSwitchEvent::Poll(PollEvent {
            ts: self.now(),
            active: Some(AccountRef::new(current.parse().ok(), roster.email(current))),
            headroom: readings
                .iter()
                .map(|(num, reading)| (num.clone(), reading.headroom))
                .collect(),
            threshold: self.settings.threshold,
            fetch_errors,
            windows,
            binding: readings
                .get(current)
                .and_then(|reading| reading.binding.clone()),
            emails: roster
                .slots
                .iter()
                .map(|slot| (slot.number.clone(), slot.identity.email.clone()))
                .collect(),
        }));
    }

    /// Nothing qualified: no readable candidate, none good enough, or every one exhausted.
    fn report_blocked(&mut self, input: &RankInput<'_>, any_known: bool) -> TickOutcome {
        if !any_known {
            self.no_switch(
                NoSwitchReason::NoComparison,
                "no candidate has readable usage",
            );
            return TickOutcome::Blocked;
        }
        // All exhausted (and its reset-aware wait) only when it's literally true: every
        // candidate known and at its limit. One that merely failed the hysteresis gate, or is
        // unreadable this tick, can become viable any moment, and the active can hit 100% and
        // need the at-limit escape, so those keep the normal cadence.
        let truly_exhausted = input.candidates.iter().all(|num| {
            input
                .headroom_of(num)
                .is_some_and(|headroom| headroom <= 0.)
        });
        if !truly_exhausted {
            self.no_switch(
                NoSwitchReason::NoQualifyingCandidate,
                "no candidate is below its switch lines and better than the active account by \
                 the hysteresis margin, or usage is unreadable this tick",
            );
            return TickOutcome::Blocked;
        }
        let earliest = earliest_recovery(input.readings, self.now());
        self.wait = earliest.map_or(Wait::Long, |earliest| Wait::Until(earliest + RESET_SLACK_S));
        self.emit(AutoSwitchEvent::AllExhausted(AllExhaustedEvent {
            ts: self.now(),
            earliest_reset_at: earliest,
        }));
        TickOutcome::Blocked
    }
}

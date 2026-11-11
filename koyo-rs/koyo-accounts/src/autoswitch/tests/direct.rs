//! cswap's direct calls of the ranking helpers (`_rank_candidates`, `_left_account_recovered`,
//! `_no_return_account`), which hand in a state, one snapshot and a headroom map as is.

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::rank::{self, RankInput};
use super::super::release;
use super::super::state::AutoSwitchState;
use super::super::windows::{Reading, Readings};
use super::{START, defaults, entry_for};
use crate::autoswitch::{AutoSwitchSettings, Trigger};
use crate::usage_store::UsageEntry;

/// A state file's contents from JSON.
pub fn state_of(value: &Value) -> AutoSwitchState {
    serde_json::from_value(value.clone()).expect("a state")
}

/// What cswap's direct calls hand in: a state, one snapshot's readings, a headroom map given as
/// is (an account it doesn't name is unknown), the active's headroom, the candidates.
pub struct Direct<'a> {
    pub settings: AutoSwitchSettings,
    pub usage: &'a [(&'a str, Value)],
    pub headroom: &'a [(&'a str, Option<f64>)],
    pub active_headroom: Option<f64>,
    pub current: &'a str,
    pub candidates: &'a [&'a str],
}

impl<'a> Direct<'a> {
    /// Defaults: no candidates, the default settings.
    pub fn new(
        usage: &'a [(&'a str, Value)],
        headroom: &'a [(&'a str, Option<f64>)],
        active_headroom: Option<f64>,
        current: &'a str,
    ) -> Direct<'a> {
        Direct {
            settings: defaults(),
            usage,
            headroom,
            active_headroom,
            current,
            candidates: &[],
        }
    }

    fn run<T>(&self, trigger: Trigger, body: impl FnOnce(&RankInput<'_>) -> T) -> T {
        let entries: BTreeMap<String, UsageEntry> = self
            .usage
            .iter()
            .map(|(num, value)| ((*num).to_owned(), entry_for(value, START)))
            .collect();
        let mut readings: Readings<'_> = entries
            .iter()
            .map(|(num, entry)| (num.clone(), Reading::of(entry, self.settings.threshold)))
            .collect();
        for reading in readings.values_mut() {
            reading.headroom = None;
        }
        for (num, headroom) in self.headroom {
            readings.entry((*num).to_owned()).or_default().headroom = *headroom;
        }
        let candidates: Vec<String> = self.candidates.iter().map(|n| (*n).to_owned()).collect();
        let input = RankInput {
            settings: &self.settings,
            trigger,
            candidates: &candidates,
            readings: &readings,
            current: self.current,
            active_headroom: self.active_headroom,
            now: START,
        };
        body(&input)
    }

    /// `_rank_candidates(trigger="proactive", ...)` with `no_return` barred.
    pub fn ranked(&self, no_return: Option<&str>) -> Vec<String> {
        self.run(Trigger::Proactive, |input| {
            rank::rank_candidates(input, no_return).0
        })
    }

    /// `_left_account_recovered(state, usage, headroom, active_headroom, settings, now, current)`.
    pub fn recovered(&self, state: &Value) -> bool {
        let state = state_of(state);
        self.run(Trigger::Proactive, |input| {
            release::left_account_recovered(&state, input)
        })
    }

    /// `_no_return_account(trigger, state, headroom, active_headroom, recovered, ...)`.
    pub fn barred(&self, trigger: Trigger, state: &Value, recovered: bool) -> Option<String> {
        let state = state_of(state);
        self.run(trigger, |input| {
            release::no_return_account(&state, input, recovered)
        })
    }
}

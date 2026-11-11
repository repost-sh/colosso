//! Part of the port of claude-swap's `autoswitch.py` (0fe22a8): the no-return bar. The engine
//! never undoes its own previous move (`_no_return_account`) unless the account it left is a
//! better proposition than when it left it (`_left_account_recovered`); the ranking is asked
//! with the bar and, when that leaves nothing and the account recovered, without it (`_rank`).
//!
//! Why: every anti-flap margin is measured against the active, and burn changes the active, so a
//! burning pair clears the margin both ways (measured, only the active burning: `[1, 2, 1, 2]`
//! where one move was due). Every return in that walk won because the active burned down, never
//! because the target recovered, so the release asks what the ranking can't: has the account we
//! left improved, on an axis burn can't fake (headroom rises only when a window rolls over, a
//! binding reset moves nearer only when a nearer window starts binding). Emptiness alone can't
//! be the release: on two accounts barring the one candidate always empties the list (measured,
//! `n=2 barred-rank EMPTY=320 NONEMPTY=0`), so it would fire every tick.
//!
//! Left out: cswap's readings of state records written before a field existed (no snapshot
//! releases; a record without `leftTrigger` is a failover when both snapshot fields are null).
//! Koyo's state file starts with every field its switches write.

use super::events::Trigger;
use super::rank::{RankInput, all_above_margin_met, rank_candidates};
use super::state::AutoSwitchState;
use super::windows::Reading;
use super::{
    HORIZON_HEADROOM_RATIO, RECOVERY_HORIZON_S, RECOVERY_HYSTERESIS_S, SPENT_HEADROOM_PCT,
};

/// The account this engine most recently left, while it is still barred.
///
/// Scoped like every sibling gate: at-limit and failover skip the anti-flap gates by design
/// (unscoped, this stranded a 2-account fleet on an exhausted active with the peer at 0%). And
/// scoped to the engine's own landing (`lastSwitchTo == current`): once the person switches by
/// hand that move is already undone and the bar only withholds the best account. A record with
/// no `lastSwitchTo` can't prove the engine moved away, so it keeps the bar.
///
/// Released when the account left now beats the active by the all-above margin
/// ([`all_above_margin_met`]), but only if it has `recovered`: the ratio compares against the
/// active, which burns, so ungated it comes true on a target that did nothing. With the active
/// unreadable, the release asks whether the ranking would land on the account left: under the
/// line.
pub(crate) fn no_return_account(
    state: &AutoSwitchState,
    input: &RankInput<'_>,
    recovered: bool,
) -> Option<String> {
    let came_from = state.last_switch_from.as_ref()?;
    if input.trigger != Trigger::Proactive {
        return None;
    }
    // As text on both sides: `lastSwitchTo` is written as a string, `lastSwitchFrom` as the
    // switch result's number, and a mismatch would disarm the bar everywhere.
    if let Some(landed_on) = &state.last_switch_to
        && landed_on.text() != input.current
    {
        return None;
    }
    let barred = came_from.text();
    if !recovered {
        return Some(barred);
    }
    if let Some(left) = input.readings.get(&barred)
        && let Some(left_headroom) = left.headroom
    {
        let released = match input.active_headroom {
            Some(active_headroom) => all_above_margin_met(left_headroom, active_headroom), // beats us outright: no flip
            None => left.lands(),
        };
        if released {
            return None;
        }
    }
    Some(barred)
}

/// Whether the account left is a better proposition than when it was left.
///
/// A failover departure (`leftTrigger` "failover") had its severity unmeasured, so there's no
/// baseline and the active's headroom can't be read either (dominance-first starved this branch
/// the moment the active fell far enough). Two legs, read against the present: the peer would be
/// a landing spot (under the line), or its binding reset is meaningfully sooner than the
/// active's. The second keeps the hold from becoming unconditional in an all-spent fleet, where
/// the first is unsatisfiable; it needs the active's reset known (`inf` for both "never" and
/// "unknown" released onto a peer arbitrarily far out) or the peer back within
/// [`RECOVERY_HORIZON_S`] (a near-spent active with a pct but no reset is alive, not unknown).
///
/// An ordinary departure has a baseline and three legs, each with its axis' margin: dominance
/// over the active (`> active × HORIZON_HEADROOM_RATIO + SPENT_HEADROOM_PCT`: the bare ratio
/// flipped true at active 1.8 purely from burn; with the margin it opens only once the active is
/// a sliver), headroom up [`SPENT_HEADROOM_PCT`] from the departure baseline, or the binding reset
/// [`RECOVERY_HYSTERESIS_S`] sooner than at departure (none, unknown or past, counts as `inf`:
/// moving off an account nobody can schedule around onto a real reset is the improvement).
pub(crate) fn left_account_recovered(state: &AutoSwitchState, input: &RankInput<'_>) -> bool {
    let Some(came_from) = &state.last_switch_from else {
        return true; // nothing was left: nothing bars
    };
    let left = input.readings.get(&came_from.text());
    let recovery = |reading: Option<&Reading<'_>>| {
        reading.map_or(f64::INFINITY, |reading| reading.reset_key(input.now))
    };
    if state.left_trigger == Some(Trigger::Failover) {
        if left.is_some_and(Reading::lands) {
            return true;
        }
        let peer_recovery = recovery(left);
        let active_recovery = recovery(input.readings.get(input.current));
        return (active_recovery.is_finite() || peer_recovery - input.now <= RECOVERY_HORIZON_S)
            && peer_recovery < active_recovery - RECOVERY_HYSTERESIS_S;
    }
    let h = left.and_then(|peer| peer.headroom);
    // Dominance over the active, for a peer that was already ahead at departure (moved for
    // another reason) and so never improves on its own baseline. An unreadable active isn't
    // "no dominance": it falls back to the landing test.
    if let Some(h) = h {
        let dominant = match input.active_headroom {
            Some(active_headroom) => {
                h > active_headroom * HORIZON_HEADROOM_RATIO + SPENT_HEADROOM_PCT
            }
            None => left.is_some_and(Reading::lands),
        };
        if dominant {
            return true;
        }
    }
    if let (Some(left_headroom), Some(h)) = (state.left_headroom, h)
        && h >= (left_headroom + SPENT_HEADROOM_PCT).min(100.)
    {
        return true;
    }
    let was = state.left_recovery_at.unwrap_or(f64::INFINITY);
    recovery(left) < was - RECOVERY_HYSTERESIS_S
}

/// The ranking with the bar, and without it when that empties the list and the account left is
/// a different proposition from the one left (cswap's `_rank`). Recomputed per snapshot.
pub(crate) fn rank_with_bar(state: &AutoSwitchState, input: &RankInput<'_>) -> (Vec<String>, bool) {
    let recovered = left_account_recovered(state, input);
    let no_return = no_return_account(state, input, recovered);
    let ranked = rank_candidates(input, no_return.as_deref());
    if no_return.is_some() && ranked.0.is_empty() && recovered {
        let unbarred = rank_candidates(input, None);
        if !unbarred.0.is_empty() {
            return unbarred;
        }
    }
    ranked
}

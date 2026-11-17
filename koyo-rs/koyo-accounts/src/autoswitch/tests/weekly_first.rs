//! cswap's `TestWeeklyFirstStrategy`, the weekly-first cases of `test_weekly_only_strategies.py`
//! (an account with no five-hour window, handled on its weekly windows alone), and of
//! `test_autoswitch_landing_room.py` the weekly-first forced move. Their cases that set a weekly
//! line or a model are left out with the settings (`best`'s escape key among them: with one line
//! below it ranks as headroom does).

use serde_json::{Value, json};

use super::*;

fn weekly_first(n: u32) -> Harness {
    Harness::numbered(n, defaults())
}

#[test]
fn holds_below_the_line_even_with_a_sooner_reset() {
    let mut h = weekly_first(3);
    let outcome = h.tick_with_usage(&[
        ("1", usage7(50., 20., Some(R_LATEST))),
        ("2", usage7(10., 10., Some(R_SOON))),
        ("3", usage7(10., 10., Some(R_LATER))),
    ]);
    assert_eq!(outcome, TickOutcome::NoAction);
    assert_eq!(h.active(), Some(1));
    assert_eq!(h.reasons(), ["below-threshold"]);
}

#[test]
fn at_the_line_takes_the_soonest_reset_over_more_headroom() {
    let mut h = weekly_first(3);
    let outcome = h.tick_with_usage(&[
        ("1", usage7(95., 20., Some(R_LATER))),
        ("2", usage7(50., 40., Some(R_SOON))),
        ("3", usage7(10., 10., Some(R_LATEST))),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    assert_eq!(h.trigger(), Trigger::Proactive);
}

#[test]
fn headroom_breaks_a_reset_tie() {
    let mut h = weekly_first(3);
    let outcome = h.tick_with_usage(&[
        ("1", usage7(95., 20., Some(R_LATER))),
        ("2", usage7(50., 40., Some(R_SOON))),
        ("3", usage7(20., 30., Some(R_SOON))),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn respects_the_hysteresis_margin() {
    let mut h = weekly_first(4);
    let outcome = h.tick_with_usage(&[
        ("1", usage7(91., 20., Some(R_LATEST))),
        ("2", usage7(85., 20., Some(R_SOON))),
        ("3", usage7(75., 20., Some(R_LATER))),
        ("4", usage7(10., 10., Some(R_LATEST))),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn skips_ineligible_sooner_resets() {
    let mut h = weekly_first(5);
    let outcome = h.tick_with_usage(&[
        ("1", usage7(95., 20., Some(R_LATEST))),
        ("2", usage7(10., 95., Some(R_SOON))),
        ("3", Value::Null),
        ("4", usage7(50., 40., Some(R_LATER))),
        ("5", usage7(5., 5., Some(R_LATEST))),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(4));
}

#[test]
fn all_over_the_lines_falls_back_to_the_most_room() {
    let mut h = weekly_first(3);
    let outcome = h.tick_with_usage(&[
        ("1", usage7(95., 50., Some(R_LATER))),
        ("2", usage7(97., 50., Some(R_SOON))),
        ("3", usage7(91., 50., Some(R_LATEST))),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn at_limit_escape_ranks_like_best() {
    let mut h = weekly_first(3);
    let outcome = h.tick_with_usage(&[
        ("1", usage7(100., 50., Some(R_LATER))),
        ("2", usage7(97., 50., Some(R_SOON))),
        ("3", usage7(60., 50., Some(R_LATEST))),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::AtLimit);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn failover_ranks_like_best() {
    let mut h = weekly_first(3);
    let fleet = [
        ("1", Value::Null),
        ("2", usage7(50., 40., Some(R_SOON))),
        ("3", usage7(10., 10., Some(R_LATEST))),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::NoAction);
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::NoAction);
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::Failover);
    assert_eq!(h.active(), Some(3));
}

/// cswap's `_harness` there: `account<n>@example.com` (cswap's runs dry; this one switches).
fn weekly_only(n: u32, settings: AutoSwitchSettings) -> Harness {
    let h = Harness::new(settings);
    for num in 1..=n {
        h.seed(num, &format!("account{num}@example.com"));
    }
    h.make_live(1);
    h
}

fn targets(h: &Harness) -> Vec<u64> {
    h.all::<SwitchEvent>()
        .iter()
        .filter_map(|event| event.to_ref.as_ref().and_then(|to| to.number))
        .collect()
}

#[test]
fn weekly_only_active_can_switch_to_weekly_only_candidate() {
    let mut h = weekly_only(3, defaults());
    let mut three = weekly(&h, 30., Some(10800.));
    three["five_hour"] = json!({"pct": 10});
    let fleet = [
        ("1", weekly(&h, 95., Some(7200.))),
        ("2", weekly(&h, 20., Some(3600.))),
        ("3", three),
    ];
    let outcome = h.tick_with_usage(&fleet);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(targets(&h), [2]);
}

#[test]
fn auto_skips_exhausted_weekly_and_unknown_candidates() {
    let mut h = weekly_only(4, defaults());
    let fleet = [
        ("1", weekly(&h, 100., Some(7200.))),
        ("2", weekly(&h, 100., Some(3600.))),
        ("3", Value::Null),
        ("4", weekly(&h, 30., Some(10800.))),
    ];
    let outcome = h.tick_with_usage(&fleet);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(targets(&h), [4]);
}

#[test]
fn a_weekly_first_forced_move_prefers_room_too() {
    let mut h = weekly_first(3);
    let outcome = h.tick_with_usage(&[
        ("1", usage7(95., 20., Some(R_LATER))),
        ("2", usage7(85., 40., Some(R_SOON))),
        ("3", usage7(10., 10., Some(R_LATEST))),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

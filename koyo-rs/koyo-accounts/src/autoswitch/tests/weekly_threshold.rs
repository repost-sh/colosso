//! cswap's `TestWeeklyThreshold`, its cases with no weekly line: every window is judged against
//! the one threshold. The cases that set a weekly line are left out with the setting.

use super::*;

#[test]
fn poll_event_omits_the_weekly_line_when_unset() {
    let mut h = Harness::fleet(2, threshold(85.));
    h.tick_with_usage(&[("1", usage7(42., 10., None)), ("2", usage7(5., 5., None))]);
    let poll = h.all::<PollEvent>().remove(0);
    assert!(poll.human().contains("5h 42% used (switch at 85%)"));
}

#[test]
fn s2_control_the_same_fleet_parks_without_a_weekly_line() {
    let mut h = Harness::fleet(2, threshold(85.));
    let outcome = h.tick_with_usage(&[("1", usage7(95., 47., None)), ("2", usage7(5., 93., None))]);
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(h.reasons(), ["no-qualifying-candidate"]);
}

#[test]
fn s4_with_weekly_unset_the_trigger_flips_where_the_scalar_did() {
    // With no weekly line every window's line is the threshold: on the boundary, on both
    // windows, and on a non-default threshold.
    for base in [90., 95.] {
        for window in ["5h", "7d"] {
            for (offset, expected) in [(-0.1, TickOutcome::NoAction), (0., TickOutcome::Switched)] {
                let mut h = Harness::fleet(2, threshold(base));
                let pct = base + offset;
                let active = if window == "5h" {
                    usage7(pct, 0., None)
                } else {
                    usage7(0., pct, None)
                };
                let outcome = h.tick_with_usage(&[("1", active), ("2", usage7(0., 0., None))]);
                assert_eq!(outcome, expected, "{base} {window} {offset}");
            }
        }
    }
}

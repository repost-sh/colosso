//! Tests translated from claude-swap's `tests/test_lines.py` (0fe22a8), named after cswap's.
//! cswap's seeded `random.Random` property sweeps run on a seeded generator of the test's own
//! (the same counts, ranges and properties; not Python's sequence).

#![expect(
    clippy::float_cmp,
    reason = "cswap's assertions are exact: the anchors are exact by construction, the point of line_headroom"
)]

use serde_json::{Value, json};

use super::*;
use crate::oauth::relevant_windows;

fn usage7(pct5: f64, pct7: f64) -> Value {
    json!({"five_hour": {"pct": pct5}, "seven_day": {"pct": pct7}})
}

fn windows(usage: &Value, models: &[&str]) -> Vec<Window> {
    relevant_windows(Some(usage), models)
}

fn binding(pct5: f64, pct7: f64, threshold: f64, weekly: Option<f64>) -> BindingWindow {
    binding_window(&windows(&usage7(pct5, pct7), &[]), threshold, weekly, None).unwrap()
}

/// splitmix64, seeded: the property sweeps' generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn uniform(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * ((self.next() >> 11) as f64 / (1u64 << 53) as f64)
    }

    fn choice<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next() % items.len() as u64) as usize]
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.).round() / 10.
}

// TestWindowLine

#[test]
fn _5h_always_takes_the_threshold() {
    assert_eq!(window_line("5h", 85., Some(97.)), 85.);
}

#[test]
fn _7d_and_scoped_take_the_weekly_line_when_set() {
    assert_eq!(window_line("7d", 85., Some(97.)), 97.);
    assert_eq!(window_line("Fable", 85., Some(97.)), 97.);
}

#[test]
fn unset_weekly_falls_back_everywhere() {
    for label in ["5h", "7d", "Fable"] {
        assert_eq!(window_line(label, 85., None), 85.);
    }
}

// TestLineHeadroom: three anchors, 0% -> 100, the line -> 100 - T, 100% -> 0.

fn grid() -> Vec<(f64, f64)> {
    let mut pairs = Vec::new();
    for t in 50..100 {
        for w in 50..100 {
            if t != w {
                pairs.push((f64::from(t), f64::from(w)));
            }
        }
    }
    pairs
}

fn decimals() -> Vec<(f64, f64)> {
    let mut pairs = Vec::new();
    for t in (500..1000).step_by(7) {
        for w in (500..1000).step_by(11) {
            if t != w {
                pairs.push((f64::from(t) / 10., f64::from(w) / 10.));
            }
        }
    }
    pairs
}

#[test]
fn equal_lines_are_raw_headroom_bit_for_bit() {
    for p in 0..=1000 {
        let pct = f64::from(p) / 10.;
        for threshold in [50., 85., 90., 97.5, 99.9] {
            assert_eq!(line_headroom(pct, threshold, threshold), 100. - pct);
        }
    }
}

#[test]
fn anchors_are_exact_on_the_integer_grid() {
    for (threshold, line) in grid() {
        assert_eq!(line_headroom(0., line, threshold), 100.);
        assert_eq!(line_headroom(line, line, threshold), 100. - threshold);
        assert_eq!(line_headroom(100., line, threshold), 0.);
    }
}

#[test]
fn line_and_exhaustion_anchors_are_exact_for_decimal_settings() {
    for (threshold, line) in decimals() {
        assert_eq!(line_headroom(line, line, threshold), 100. - threshold);
        assert_eq!(line_headroom(100., line, threshold), 0.);
        assert!((line_headroom(0., line, threshold) - 100.).abs() < 1e-9);
    }
}

#[test]
fn values_under_and_over_the_weekly_line() {
    assert!((line_headroom(93., 97., 85.) - (15. + 85. * 4. / 97.)).abs() < 1e-9);
    assert_eq!(line_headroom(98., 97., 85.), 10.);
    assert_eq!(line_headroom(99., 97., 85.), 5.);
}

#[test]
fn a_weekly_line_below_the_5h_line() {
    assert!((line_headroom(70., 80., 90.) - (10. + 90. * 10. / 80.)).abs() < 1e-9);
    assert_eq!(line_headroom(85., 80., 90.), 7.5);
    assert_eq!(line_headroom(100., 80., 90.), 0.);
}

#[test]
fn the_exact_boundary_that_a_rescale_missed() {
    assert_eq!(line_headroom(86., 86., 92.), 8.);
    assert_eq!(line_headroom(82., 92., 92.), 18.);
}

#[test]
fn monotone_in_each_windows_pct() {
    let mut rng = Rng(7);
    for _ in 0..20_000 {
        let threshold = rng.choice(&[50., 85., 90., 97.5]);
        let weekly = rng.choice(&[50., 80., 97., 99.9]);
        let (pct5, pct7) = (rng.uniform(0., 100.), rng.uniform(0., 100.));
        let step = rng.uniform(0., 5.);
        let before = binding(pct5, pct7, threshold, Some(weekly)).line_headroom;
        let up5 = binding((pct5 + step).min(100.), pct7, threshold, Some(weekly)).line_headroom;
        let up7 = binding(pct5, (pct7 + step).min(100.), threshold, Some(weekly)).line_headroom;
        assert!(up5 <= before + 1e-12 && up7 <= before + 1e-12);
    }
}

// TestBindingWindow

#[test]
fn least_policy_headroom_binds_not_the_highest_pct() {
    let found = binding(5., 93., 85., Some(97.));
    assert_eq!(
        (found.label.as_str(), found.pct, found.line, found.over()),
        ("7d", 93., 97., false)
    );
    let found = binding(95., 47., 85., Some(97.));
    assert_eq!((found.label.as_str(), found.over()), ("5h", true));
}

#[test]
fn over_is_the_direct_comparison_at_the_line() {
    assert!(binding(5., 97., 85., Some(97.)).over());
    assert!(!binding(5., 96.9, 85., Some(97.)).over());
    assert!(binding(85., 5., 85., Some(97.)).over());
}

#[test]
fn ties_keep_the_first_window() {
    assert_eq!(binding(0., 0., 90., None).label, "5h");
    assert_eq!(binding(50., 50., 90., Some(90.)).label, "5h");
}

#[test]
fn no_windows_gives_none() {
    assert_eq!(binding_window(&[], 85., Some(97.), None), None);
    assert_eq!(
        binding_window(&relevant_windows(None, &[]), 85., None, None),
        None
    );
}

#[test]
fn resets_at_travels_with_the_window() {
    let usage = json!({
        "five_hour": {"pct": 90.0, "resets_at": "2026-01-01T01:00:00Z"},
        "seven_day": {"pct": 93.0, "resets_at": "2026-01-05T00:00:00Z"},
    });
    let found = binding_window(&windows(&usage, &[]), 85., Some(97.), None).unwrap();
    assert_eq!(found.label, "5h");
    assert_eq!(found.resets_at.as_deref(), Some("2026-01-01T01:00:00Z"));
}

#[test]
fn scoped_windows_take_the_weekly_line() {
    let usage = json!({
        "five_hour": {"pct": 5.0},
        "seven_day": {"pct": 10.0},
        "scoped": [{"name": "Fable", "pct": 96.0}],
    });
    let found = binding_window(&windows(&usage, &["Fable"]), 85., Some(97.), None).unwrap();
    assert_eq!(
        (found.label.as_str(), found.line, found.over()),
        ("Fable", 97., false)
    );
    let ignored = binding_window(&windows(&usage, &[]), 85., Some(97.), None).unwrap();
    assert_eq!(ignored.label, "7d");
}

#[test]
fn weekly_unset_reproduces_the_max_pct_rule_exactly() {
    let mut rng = Rng(0);
    for _ in 0..20_000 {
        let threshold = rng.choice(&[50., 85., 90., 97.5, 99.9]);
        let pct5 = round1(rng.uniform(0., 100.));
        let pct7 = round1(rng.uniform(0., 100.));
        let found = binding(pct5, pct7, threshold, None);
        assert_eq!(found.pct, pct5.max(pct7));
        assert_eq!(found.label, if pct5 >= pct7 { "5h" } else { "7d" });
        assert_eq!(
            Some(found.line_headroom),
            crate::oauth::account_headroom(Some(&usage7(pct5, pct7)), &[])
        );
        assert_eq!(found.over(), pct5.max(pct7) >= threshold);
    }
}

#[test]
fn over_matches_any_window_and_headroom_matches_the_minimum() {
    let mut rng = Rng(1);
    for _ in 0..20_000 {
        let threshold = rng.choice(&[50., 85., 90., 97.5]);
        let weekly = rng.choice(&[None, Some(80.), Some(97.), Some(99.9)]);
        let pct5 = round1(rng.uniform(0., 100.));
        let pct7 = round1(rng.uniform(0., 100.));
        let found = binding(pct5, pct7, threshold, weekly);
        let line7 = window_line("7d", threshold, weekly);
        assert_eq!(found.over(), pct5 >= threshold || pct7 >= line7);
        assert_eq!(
            found.line_headroom,
            line_headroom(pct5, threshold, threshold).min(line_headroom(pct7, line7, threshold))
        );
    }
}

#[test]
fn the_binding_flip_is_continuous() {
    assert_eq!(binding(86.7, 99., 85., Some(97.)).line_headroom, 5.);
    assert_eq!(binding(86.8, 99., 85., Some(97.)).line_headroom, 5.);
}

// TestBindingWindowScale

fn window() -> [Window; 1] {
    [Window::new("5h", 50., Some("2024-01-05T00:00:00Z"))]
}

#[test]
fn a_scale_scores_the_room_on_that_ruler() {
    let found = binding_window(&window(), 60., None, Some(90.)).unwrap();
    assert_eq!(found.line_headroom, 25.);
    assert_eq!(found.line_headroom, line_headroom(50., 60., 90.));
}

#[test]
fn no_scale_is_the_line_as_its_own_ruler() {
    assert_eq!(
        binding_window(&window(), 60., None, None)
            .unwrap()
            .line_headroom,
        50.
    );
}

#[test]
fn a_scale_equal_to_the_line_is_plain_headroom() {
    assert_eq!(
        binding_window(&window(), 60., None, Some(60.))
            .unwrap()
            .line_headroom,
        50.
    );
}

#[test]
fn scale_moves_only_the_room() {
    let plain = binding_window(&window(), 60., None, None).unwrap();
    for scale in [50., 85., 90., 95.] {
        let found = binding_window(&window(), 60., None, Some(scale)).unwrap();
        assert_eq!(
            (
                &found.label,
                found.pct,
                found.line,
                &found.resets_at,
                found.over()
            ),
            (
                &plain.label,
                plain.pct,
                plain.line,
                &plain.resets_at,
                plain.over()
            )
        );
    }
}

#[test]
fn the_scale_can_change_which_window_binds() {
    let windows = windows(&usage7(73.3, 94.), &[]);
    assert_eq!(
        binding_window(&windows, 55.5, Some(90.), Some(90.))
            .unwrap()
            .label,
        "5h"
    );
    assert_eq!(
        binding_window(&windows, 55.5, Some(90.), None)
            .unwrap()
            .label,
        "7d"
    );
}

// TestLineRoom

#[test]
fn each_window_against_its_own_line() {
    assert_eq!(
        line_room(&windows(&usage7(80., 88.), &[]), 90., Some(98.)),
        Some(10.)
    );
    assert_eq!(
        line_room(&windows(&usage7(50., 89.), &[]), 90., Some(98.)),
        Some(9.)
    );
}

#[test]
fn not_the_5h_scale() {
    let windows98 = windows(&usage7(0., 88.), &[]);
    let found = binding_window(&windows98, 90., Some(98.), None).unwrap();
    assert_eq!(((found.line_headroom - 10.) * 100.).round() / 100., 9.18);
    assert_eq!(line_room(&windows98, 90., Some(98.)), Some(10.));
    let windows60 = windows(&usage7(0., 52.), &[]);
    let found = binding_window(&windows60, 60., None, Some(90.)).unwrap();
    assert_eq!(found.line_headroom - 10., 12.);
    assert_eq!(line_room(&windows60, 60., None), Some(8.));
}

#[test]
fn not_only_the_binding_window() {
    let windows = windows(&usage7(81., 88.5), &[]);
    let found = binding_window(&windows, 90., Some(98.), None).unwrap();
    assert_eq!(found.label, "7d");
    assert_eq!(line_room(&windows, 90., Some(98.)), Some(9.));
}

#[test]
fn negative_once_over() {
    assert_eq!(
        line_room(&windows(&usage7(92., 10.), &[]), 90., Some(98.)),
        Some(-2.)
    );
}

#[test]
fn no_window_is_none() {
    assert_eq!(line_room(&[], 90., Some(98.)), None);
}

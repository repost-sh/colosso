//! cswap's `TestEventsShape` and `TestPctLabel`, with `TestWeeklyThreshold`'s poll sentences.
//! cswap's JSON envelope (`to_json`) is left out; its `ts` and kind are asserted on the events.

use serde_json::json;

use super::super::events::pct_label;
use super::*;
use crate::lines::BindingWindow;

fn poll(active: Option<AccountRef>, headroom: &[(&str, f64)], threshold: f64) -> PollEvent {
    PollEvent {
        ts: START,
        active,
        headroom: headroom
            .iter()
            .map(|(num, headroom)| ((*num).to_owned(), Some(*headroom)))
            .collect(),
        threshold,
        fetch_errors: BTreeMap::new(),
        windows: BTreeMap::new(),
        binding: None,
        emails: BTreeMap::new(),
    }
}

fn binding(label: &str, pct: f64, line: f64) -> BindingWindow {
    BindingWindow {
        label: label.to_owned(),
        pct,
        line,
        resets_at: None,
        line_headroom: 100. - pct,
    }
}

fn account_a() -> AccountRef {
    AccountRef {
        number: Some(1),
        email: "a@example.com".into(),
    }
}

#[test]
fn every_event_has_envelope() {
    let mut h = Harness::standard();
    h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.)), ("3", usage(50.))]);
    let events = h.events();
    assert!(!events.is_empty());
    for event in events {
        assert_exact(event.ts(), START);
        assert!(!event.kind().is_empty());
    }
}

#[test]
fn switch_event_refs_match_account_ref_shape() {
    let mut h = Harness::standard();
    h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.)), ("3", usage(50.))]);
    let switch = h.all::<SwitchEvent>().remove(0);
    assert_eq!(switch.from_ref, Some(account_a()));
    assert_eq!(
        switch.to_ref,
        Some(AccountRef {
            number: Some(2),
            email: "b@example.com".into()
        })
    );
    assert_eq!(
        AutoSwitchEvent::Switch(switch).human(),
        "Switched a@example.com -> b@example.com (proactive)"
    );
}

#[test]
fn poll_event_human_line() {
    let mut h = Harness::standard();
    h.tick_with_usage(&[("1", usage(42.)), ("2", usage(10.)), ("3", Value::Null)]);
    let line = h.all::<PollEvent>().remove(0).human();
    assert!(
        line.contains("a@example.com") && line.contains("42% used"),
        "{line}"
    );
    // The others show each window, not just the ambiguous binding pct.
    assert!(line.contains("b@example.com: 5h 10% · 7d 0%"), "{line}");
    assert!(line.contains("c@example.com: ?"), "{line}");
}

#[test]
fn poll_event_windows_match_the_decision_set() {
    // A model's window isn't one the decision reads: an ignored Fable 100% next to a switch
    // onto that account would read as a bug.
    let fleet = [
        ("1", usage(42.)),
        (
            "2",
            json!({"five_hour": {"pct": 3.0}, "seven_day": {"pct": 89.0},
                   "scoped": [{"name": "Fable", "pct": 21.0}]}),
        ),
    ];
    let mut plain = Harness::fleet(2, defaults());
    plain.tick_with_usage(&fleet);
    let event = plain.all::<PollEvent>().remove(0);
    assert!(event.human().contains("b@example.com: 5h 3% · 7d 89%"));
    assert!(!event.human().contains("Fable"));
    assert_eq!(
        event.windows["2"],
        [("5h".to_owned(), 3.), ("7d".to_owned(), 89.)]
    );
}

#[test]
fn whole_numbers_drop_the_decimal() {
    assert_eq!(pct_label(90.), "90");
}

#[test]
fn fractional_threshold_keeps_one_decimal() {
    // `.0f` would render the valid maximum 99.9 as a lying "100".
    assert_eq!(pct_label(99.9), "99.9");
}

#[test]
fn configured_precision_is_preserved() {
    assert_eq!(pct_label(85.55), "85.55");
    assert_eq!(pct_label(85.555555), "85.555555");
}

#[test]
fn float_noise_is_absorbed() {
    assert_eq!(pct_label(100. - 37.4), "62.6");
    assert_eq!(pct_label(99.850_000_000_000_01), "99.85");
}

#[test]
fn poll_event_shows_fractional_threshold() {
    let event = poll(Some(account_a()), &[("1", 40.)], 99.9);
    assert!(event.human().contains("switch at 99.9%"));
}

#[test]
fn below_threshold_detail_shows_fractional_threshold() {
    let mut h = Harness::fleet(2, threshold(99.9));
    h.tick_with_usage(&[("1", usage(50.)), ("2", usage(10.))]);
    let details: Vec<String> = h
        .all::<NoSwitchEvent>()
        .into_iter()
        .map(|e| e.detail)
        .collect();
    assert_eq!(details, ["5h 50% < 99.9%"]);
}

#[test]
fn below_threshold_detail_never_shows_impossible_comparison() {
    // 99.85 under 99.9: `.0f` on the left rendered the impossible "100% < 99.9%".
    let mut h = Harness::fleet(2, threshold(99.9));
    h.tick_with_usage(&[("1", usage(99.85)), ("2", usage(10.))]);
    let details: Vec<String> = h
        .all::<NoSwitchEvent>()
        .into_iter()
        .map(|e| e.detail)
        .collect();
    assert_eq!(details, ["5h 99.85% < 99.9%"]);
}

#[test]
fn poll_event_human_names_the_binding_window_and_its_line() {
    // The binding window the poll measured, and its line.
    let mut measured = poll(Some(account_a()), &[("1", 7.)], 85.);
    measured.binding = Some(binding("7d", 93., 97.));
    assert!(measured.human().contains("7d 93% used (switch at 97%)"));
    // No window data (sentinels): the account-wide form.
    let bare = poll(Some(account_a()), &[("1", 7.)], 85.);
    assert!(bare.human().contains("93% used (switch at 85%)"));
    // From a tick: the binding window is the one the trigger compared.
    let mut h = Harness::fleet(2, defaults());
    h.tick_with_usage(&[("1", usage7(42., 10., None)), ("2", usage(5.))]);
    let event = h.all::<PollEvent>().remove(0);
    assert_eq!(event.binding, Some(binding("5h", 42., 90.)));
    assert!(
        event
            .human()
            .contains("a@example.com: 5h 42% used (switch at 90%)")
    );
}

#[test]
fn the_other_events_say_what_happened() {
    // The sentences the log shows (cswap's `human()`), for the events no other test reads.
    let quarantined = AutoSwitchEvent::Quarantine(QuarantineEvent {
        ts: START,
        number: "2".into(),
        email: "b@example.com".into(),
        reason: QuarantineReason::InvalidGrant,
    });
    assert!(
        quarantined
            .human()
            .starts_with("b@example.com quarantined: invalid_grant.")
    );
    let back = AutoSwitchEvent::Unquarantine(UnquarantineEvent {
        ts: START,
        number: "2".into(),
        email: String::new(),
        reason: ReleaseReason::CredentialsReplaced,
    });
    assert_eq!(
        back.human(),
        "Account-2 back in rotation (credentials-replaced)"
    );
    let exhausted = AutoSwitchEvent::AllExhausted(AllExhaustedEvent {
        ts: START,
        earliest_reset_at: None,
    });
    assert_eq!(
        exhausted.human(),
        "all accounts exhausted; no reset time known"
    );
    let exhausted = AutoSwitchEvent::AllExhausted(AllExhaustedEvent {
        ts: START,
        earliest_reset_at: Some(START + 0.5),
    });
    assert_eq!(
        exhausted.human(),
        "all accounts exhausted; earliest reset 1970-01-12T13:46:40.500000Z"
    );
    let error = AutoSwitchEvent::Error(ErrorEvent {
        ts: START,
        message: "boom".into(),
    });
    assert_eq!(error.human(), "error: boom (will retry)");
    let held = AutoSwitchEvent::NoSwitch(NoSwitchEvent {
        ts: START,
        reason: NoSwitchReason::Cooldown,
        detail: String::new(),
    });
    assert_eq!(held.human(), "no switch: cooldown");
    assert_eq!(
        AutoSwitchEvent::Poll(poll(None, &[], 90.)).human(),
        "poll: no active account"
    );
}

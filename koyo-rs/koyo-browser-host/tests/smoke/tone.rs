//! A page says its tone as it first shows and as it changes, nothing while it's left alone or
//! hidden, and what changed meanwhile once it's shown again.

use koyo_browser_ipc::{AppRequest, HostEvent, Tone};
use std::time::{Duration, Instant};

use crate::agents::cdp;
use crate::support::{Session, VIEWPORT, close, launch, until};

/// The tones pane 1's page says until `done` holds of an event, or `within` is over.
fn told(
    session: &mut Session,
    within: Duration,
    mut done: impl FnMut(&HostEvent, &[Tone]) -> bool,
) -> Vec<Tone> {
    let mut tones = Vec::new();
    session.pump(Instant::now() + within, |event| {
        if let HostEvent::Tone { pane: 1, tone } = event {
            tones.push(tone.clone());
        }
        done(event, &tones)
    });
    tones
}

fn all(tone: &Tone, light: impl Fn(f32) -> bool) -> bool {
    tone.lightness.len() == Tone::ACROSS * Tone::DOWN && tone.lightness.iter().all(|cell| light(*cell))
}

/// Paints pane 1's page `colour`, as the agent's command `call`.
fn paint(session: &mut Session, call: u64, colour: &str) {
    let expression = format!("document.body.style.background = '{colour}'");
    cdp(session, call, "Runtime.evaluate", serde_json::json!({ "expression": expression }));
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_says_its_tone_as_it_shows_and_changes_and_nothing_left_alone() {
    let launch = launch("tone");
    let mut session = Session::start(&launch);
    let page = "data:text/html,<title>Tone</title><body style='margin:0;background:rgb(255,255,255)'>still</body>";
    let open = AppRequest::Open { pane: 1, url: page.into(), viewport: VIEWPORT, profile: "smoke".into() };
    session.host.send(&open).expect("open");
    let mut loaded = false;
    let first = told(&mut session, Duration::from_secs(30), |event, tones| {
        loaded |= matches!(event, HostEvent::Page { pane: 1, state } if state.title == "Tone" && !state.loading);
        loaded && !tones.is_empty()
    });
    let white = first.last().expect("the page said its tone as it showed");
    assert!(all(white, |cell| cell > 0.95) && white.canvas == [255; 3], "{white:?}");

    let alone = told(&mut session, Duration::from_secs(3), |_, _| false);
    assert_eq!(alone, [], "a page left alone said its tone again");

    paint(&mut session, 1, "rgb(0, 0, 0)");
    let dark = told(&mut session, Duration::from_secs(10), |_, tones| !tones.is_empty());
    let dark = dark.last().expect("the page said it turned dark");
    assert!(all(dark, |cell| cell < 0.05) && dark.canvas == [0; 3], "{dark:?}");

    session.host.send(&AppRequest::Sight { pane: 1, kept: false, seen: true }).expect("hide");
    let state = "JSON.stringify(document.visibilityState)";
    until(&mut session, state, "the page hidden", Duration::from_secs(10), |state| state == "hidden");
    paint(&mut session, 2, "rgb(255, 255, 255)");
    let hidden = told(&mut session, Duration::from_secs(3), |_, _| false);
    assert_eq!(hidden, [], "a hidden page said its tone");

    session.host.send(&AppRequest::Sight { pane: 1, kept: true, seen: true }).expect("show");
    let shown = told(&mut session, Duration::from_secs(10), |_, tones| !tones.is_empty());
    let shown = shown.last().expect("shown again, the page said what changed meanwhile");
    assert!(all(shown, |cell| cell > 0.95), "{shown:?}");
    close(session, &launch);
}

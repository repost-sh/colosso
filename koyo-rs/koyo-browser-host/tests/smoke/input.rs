//! The person's input as a page sees it: each event at the time it was played. The engine
//! measures from an input's time (how long a click took to answer, when a click that goes to
//! another view of the page started), and stops a page whose click is older than the page.

use koyo_browser_ipc::{AppRequest, KeyKind, Modifiers, MouseButton, MouseKind};
use std::time::Duration;

use crate::support::{Session, close, evaluate, launch, open, serve, until};

/// How long the page has to show what a test waits for.
const SHOWN: Duration = Duration::from_secs(10);

/// How long a page is left alone before input is played to it, in its own milliseconds: an
/// input stamped with anything but the time it's played is that far off, or further.
const SETTLED: f64 = 1500.;

/// A page that keeps, for each kind of input, when the event says it happened and when the
/// page got it.
const TIMES: &str = "data:text/html,<title>times</title><body style='margin:0;height:3000px'><script>\
    var times = {};\
    for (const type of ['mousemove', 'mousedown', 'mouseup', 'wheel', 'keydown', 'keyup'])\
      addEventListener(type, event => { times[type] = [event.timeStamp, performance.now()]; }, { passive: true });\
    </script>";

/// A page whose button goes to another view of it as an app's link does: a new address in its
/// history and new content, without a load. It keeps when the click says it happened, and
/// when the engine says the navigation within the page started.
const APP: &str = "<!doctype html><title>home</title><body style='margin:0'>\
    <button id='go' style='width:100vw;height:100vh'>Go</button><script>\
    var clicked = null, navigated = null;\
    new PerformanceObserver(list => { navigated = list.getEntries()[0].startTime; })\
      .observe({ type: 'soft-navigation', buffered: true });\
    go.onclick = event => {\
      clicked = event.timeStamp;\
      history.pushState({}, '', '/next');\
      document.body.innerHTML = '<h1>' + 'The next view, with text enough to fill it. '.repeat(40) + '</h1>';\
      document.title = 'next';\
    };</script>";

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn input_reaches_the_page_stamped_with_the_time_it_was_played() {
    let launch = launch("input-times");
    let mut session = Session::start(&launch);
    open(&mut session, TIMES, "times");
    settle(&mut session);
    session
        .host
        .send(&AppRequest::Focus {
            pane: 1,
            focused: true,
        })
        .expect("focus");
    click(&mut session);
    session
        .host
        .send(&AppRequest::Wheel {
            pane: 1,
            x: 200.,
            y: 150.,
            dx: 0.,
            dy: -40.,
            modifiers: Modifiers::default(),
        })
        .expect("wheel");
    for kind in [KeyKind::Down, KeyKind::Char, KeyKind::Up] {
        session
            .host
            .send(&AppRequest::Key {
                pane: 1,
                kind,
                key_code: 0x41,
                native_key_code: 0x00,
                text: Some("a".into()),
                modifiers: Modifiers::default(),
            })
            .expect("key");
    }
    let times = until(
        &mut session,
        "JSON.stringify(times)",
        "every kind of input to reach the page",
        SHOWN,
        |times| times.as_object().is_some_and(|times| times.len() == 6),
    );
    for (kind, time) in times.as_object().expect("an object") {
        let (stamped, got) = (number(&time[0]), number(&time[1]));
        assert!(
            stamped >= SETTLED && (got - stamped).abs() < 1000.,
            "a {kind} the page got at {got} ms says it happened at {stamped} ms"
        );
    }
    close(session, &launch);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_click_that_goes_to_another_view_of_the_page_keeps_it_running() {
    let launch = launch("input-soft-navigation");
    let mut session = Session::start(&launch);
    open(&mut session, &serve(APP), "home");
    settle(&mut session);
    click(&mut session);
    // The engine takes the click for a navigation once the new view has painted, and counts
    // the navigation from the click: it stops a page whose click is older than the page.
    let times = until(
        &mut session,
        "JSON.stringify([clicked, navigated, document.title, location.pathname])",
        "the engine to take the click for a navigation within the page",
        SHOWN,
        |times| !times[1].is_null(),
    );
    let (clicked, navigated) = (number(&times[0]), number(&times[1]));
    assert!(
        clicked >= SETTLED && (navigated - clicked).abs() < 1000.,
        "a click stamped {clicked} ms started a navigation the engine counts from {navigated} ms"
    );
    assert_eq!(
        (times[2].as_str(), times[3].as_str()),
        (Some("next"), Some("/next"))
    );
    close(session, &launch);
}

/// Leaves pane 1's page alone until it is [`SETTLED`] old and has painted.
fn settle(session: &mut Session) {
    let wait = format!(
        "new Promise(done => setTimeout(() => requestAnimationFrame(() => done(true)), \
         Math.max(0, {SETTLED} - performance.now())))"
    );
    assert_eq!(evaluate(session, &wait).as_bool(), Some(true));
}

/// A click in the middle of pane 1, as the app plays one: the pointer moves there, the button
/// goes down and comes up.
fn click(session: &mut Session) {
    for (kind, button) in [
        (MouseKind::Move, None),
        (MouseKind::Down, Some(MouseButton::Left)),
        (MouseKind::Up, Some(MouseButton::Left)),
    ] {
        session
            .host
            .send(&AppRequest::Mouse {
                pane: 1,
                kind,
                x: 200.,
                y: 150.,
                button,
                click_count: 1,
                modifiers: Modifiers::default(),
            })
            .expect("mouse");
    }
}

fn number(value: &serde_json::Value) -> f64 {
    value.as_f64().expect("a number")
}

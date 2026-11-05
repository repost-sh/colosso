//! The person's input as a page sees it: each event at the time it was played. The engine
//! measures from an input's time (how long a click took to answer, when a click that goes to
//! another view of the page started), and stops a page whose click is older than the page.

use koyo_browser_ipc::{
    AppRequest, HostEvent, KeyKind, Modifiers, MouseButton, MouseKind, Viewport,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use crate::agents::cdp;
use crate::support::{Session, launch};

const VIEWPORT: Viewport = Viewport {
    width: 400,
    height: 300,
    scale: 1.,
};

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

/// Opens `url` as pane 1 and waits for it to load, `title` being its title.
fn open(session: &mut Session, url: &str, title: &str) {
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: url.into(),
            viewport: VIEWPORT,
            profile: "smoke-input".into(),
        })
        .expect("open");
    session.wait("the page to load", |event, _| {
        matches!(event, HostEvent::Page { pane: 1, state } if state.title == title && !state.loading)
    });
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

/// The JSON `expression` makes in pane 1's page, once `settled` holds of it: asked again every
/// 50 ms of the page's until then.
fn until(
    session: &mut Session,
    expression: &str,
    what: &str,
    settled: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let later = format!("new Promise(done => setTimeout(() => done({expression}), 50))");
    for _ in 0..200 {
        let text = evaluate(session, &later);
        let value = serde_json::from_str(text.as_str().expect("JSON text")).expect("JSON");
        if settled(&value) {
            return value;
        }
    }
    panic!("timed out waiting for {what}");
}

/// What `expression` comes to in pane 1's page, a promise awaited. A page that stops
/// meanwhile fails the test with why.
fn evaluate(session: &mut Session, expression: &str) -> serde_json::Value {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let params = serde_json::json!({
        "expression": expression,
        "returnByValue": true,
        "awaitPromise": true,
    });
    cdp(session, call, "Runtime.evaluate", params);
    let mut answer = None;
    session.wait("the page's answer", |event, _| match event {
        HostEvent::Gone { reason, .. } => panic!("the page stopped: {reason}"),
        HostEvent::CdpReply {
            call: answered,
            result,
            error,
            ..
        } if *answered == call => {
            answer = Some((result.clone(), error.clone()));
            true
        }
        _ => false,
    });
    let (result, error) = answer.expect("answered");
    assert_eq!(error, None, "{expression}");
    let result = result.expect("a result");
    assert!(
        result["exceptionDetails"].is_null(),
        "{expression}: {result}"
    );
    result["result"]["value"].clone()
}

fn number(value: &serde_json::Value) -> f64 {
    value.as_f64().expect("a number")
}

fn close(session: Session, launch: &koyo_browser_ipc::client::Launch) {
    session.host.shutdown(Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

/// Serves `page` at every path of a local address, which a page needs to change its own:
/// the address.
fn serve(page: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let address = format!("http://{}/", listener.local_addr().expect("an address"));
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|end| end == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(read) if read > 0 => request.extend_from_slice(&chunk[..read]),
                    _ => break,
                }
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
        }
    });
    address
}

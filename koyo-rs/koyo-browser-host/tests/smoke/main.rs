//! End to end against a packaged engine: start the host as a process of its own, where its
//! pages are in holders parked off the screen, open a page, and ask it things. The host never
//! comes forward (`mac_app.rs`), so they run while the person works.
//!
//! They wait for an engine `script/browser-engine` packaged from the host just built, which
//! `script/check` packages before it runs them; `KOYO_BROWSER_ENGINE` names another:
//!
//! ```sh
//! script/browser-engine
//! script/cargo test -p koyo-browser-host --test smoke -- --ignored --skip location::
//! ```
//!
//! The location test waits for the person to answer macOS's prompt the first time, so
//! `script/check` leaves it out.
#![cfg(target_os = "macos")]
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

mod agents;
mod extensions;
mod location;
mod media;
mod passkeys;
mod popups;
mod saves;
mod sight;
mod support;
mod tone;

use koyo_browser_ipc::client::Connection;
use koyo_browser_ipc::{AppRequest, HostEvent, Viewport};
use std::time::{Duration, Instant};
use support::{Session, close, evaluate, launch, open, until};

/// How many animation frames pane 1's page gets in half a second.
const FRAMES: &str = "new Promise(done => { let frames = 0; \
    const count = () => { frames += 1; requestAnimationFrame(count); }; \
    requestAnimationFrame(count); setTimeout(() => done(frames), 500); })";

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_opens_at_its_size_draws_while_on_screen_and_rests_out_of_sight() {
    let launch = launch("page");
    let mut session = Session::start(&launch);
    assert!(session.host.chromium.starts_with("156."), "{}", session.host.chromium);
    open(&mut session, "data:text/html,<title>page</title><body>hi</body>", "page");
    let size = "JSON.stringify([innerWidth, innerHeight])";
    let is = |width: u64, height: u64| {
        move |size: &serde_json::Value| *size == serde_json::json!([width, height])
    };
    until(&mut session, size, "the page at its size", Duration::from_secs(10), is(400, 300));
    let drawn = evaluate(&mut session, FRAMES).as_u64().expect("a count");
    assert!(drawn > 15, "{drawn} frames in half a second on screen");
    let visible = |session: &mut Session, visible| {
        (session.host.send(&AppRequest::Visible { pane: 1, visible })).expect("visible");
    };
    visible(&mut session, false);
    until(
        &mut session,
        "JSON.stringify(document.visibilityState)",
        "the page to be hidden",
        Duration::from_secs(10),
        |state| state == "hidden",
    );
    visible(&mut session, true);
    until(
        &mut session,
        "JSON.stringify(document.visibilityState)",
        "the page to show again",
        Duration::from_secs(10),
        |state| state == "visible",
    );
    let drawn = evaluate(&mut session, FRAMES).as_u64().expect("a count");
    assert!(drawn > 15, "{drawn} frames in half a second back on screen");
    let viewport = Viewport {
        width: 390,
        height: 844,
        scale: 2.,
    };
    (session.host.send(&AppRequest::Resize { pane: 1, viewport })).expect("resize");
    until(&mut session, size, "the page at the size told", Duration::from_secs(10), is(390, 844));
    let (pid, quitting) = (session.host.pid(), Instant::now());
    close(session, &launch);
    let quit = quitting.elapsed();
    eprintln!("host {pid} quit in {quit:?}");
    assert!(
        quit < Duration::from_secs(5),
        "the host quits on request, not when killed"
    );
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn dev_tools_open_as_a_pane_of_their_own_and_close_with_their_page() {
    let launch = launch("devtools");
    let mut session = Session::start(&launch);
    open(
        &mut session,
        "data:text/html,<title>Inspected</title><body><p>hi</p></body>",
        "Inspected",
    );
    let tools = |session: &mut Session, devtools, inspect| {
        (session.host.send(&AppRequest::OpenDevTools {
            pane: 1,
            devtools,
            viewport: support::VIEWPORT,
            inspect,
            dock: koyo_browser_ipc::DevToolsDock::Bottom,
        }))
        .expect("open devtools");
    };
    tools(&mut session, 2, Some(koyo_browser_ipc::Point { x: 4., y: 4. }));
    session.wait("the DevTools page", |event| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.url.starts_with("devtools://"))
    });
    session.wait("the inspector to attach to the page", |event| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.title.starts_with("DevTools - data:text/html"))
    });
    tools(&mut session, 3, None);
    session.wait("the second inspector to be refused", |event| {
        matches!(event, HostEvent::Error { pane: Some(3), .. })
    });
    (session.host.send(&AppRequest::Close { pane: 2 })).expect("close devtools");
    tools(&mut session, 4, None);
    session.wait("the new inspector to attach to the page", |event| {
        matches!(event, HostEvent::Page { pane: 4, state } if state.title.starts_with("DevTools - data:text/html"))
    });
    (session.host.send(&AppRequest::Close { pane: 1 })).expect("close");
    let mut closed = Vec::new();
    session.wait("the page and its inspector to close", |event| {
        if let HostEvent::Closed { pane } = event {
            closed.push(*pane);
        }
        closed.contains(&1) && closed.contains(&4)
    });
    close(session, &launch);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_goes_full_screen_and_back_and_the_host_still_quits() {
    let launch = launch("fullscreen");
    let mut session = Session::start(&launch);
    let page = "data:text/html,<title>small</title><body style='margin:0'>\
        <script>document.onfullscreenchange=()=>\
        document.title=document.fullscreenElement?'full':'small'</script>";
    open(&mut session, page, "small");
    agents::cdp(
        &mut session,
        900,
        "Runtime.evaluate",
        serde_json::json!({"expression": "document.body.requestFullscreen()", "userGesture": true}),
    );
    session.wait("the page to go full screen", |event| {
        matches!(event, HostEvent::Fullscreen { pane: 1, on: true })
    });
    (session.host.send(&AppRequest::ExitFullscreen { pane: 1 })).expect("leave");
    session.wait("the page to leave full screen", |event| {
        matches!(event, HostEvent::Fullscreen { pane: 1, on: false })
    });
    (session.host.send(&AppRequest::Close { pane: 1 })).expect("close");
    session.wait("the page to close", |event| matches!(event, HostEvent::Closed { pane: 1 }));
    let quitting = Instant::now();
    close(session, &launch);
    assert!(
        quitting.elapsed() < Duration::from_secs(5),
        "the host quits on request after a page was full screen: {:?}",
        quitting.elapsed()
    );
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_whose_renderer_crashes_is_gone_and_the_host_serves_on() {
    let launch = launch("crash");
    let mut session = Session::start(&launch);
    open(&mut session, "data:text/html,<title>doomed</title>", "doomed");
    agents::cdp(&mut session, 900, "Page.crash", serde_json::json!({}));
    let mut gone = None;
    session.wait("the page to be gone", |event| match event {
        HostEvent::Gone { pane: 1, reason } => {
            gone = Some(reason.clone());
            true
        }
        _ => false,
    });
    let reason = gone.expect("a reason");
    assert!(!reason.is_empty(), "the person is told why the page went");
    (session.host)
        .send(&AppRequest::Open {
            pane: 2,
            url: "data:text/html,<title>next</title>".into(),
            viewport: support::VIEWPORT,
            profile: "smoke".into(),
        })
        .expect("open");
    session.wait("the next page to load", |event| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.title == "next" && !state.loading)
    });
    close(session, &launch);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_host_left_holding_the_profiles_gives_way_to_the_next() {
    let launch = launch("left");
    let first = Session::start(&launch);
    let stuck = first.host.pid();
    let stopped = std::process::Command::new("kill")
        .args(["-STOP", &stuck.to_string()])
        .status()
        .expect("kill");
    assert!(stopped.success());
    let started = Instant::now();
    let next = Connection::start(&launch, |_| None).expect("the next host starts");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    assert_ne!(next.pid(), stuck);
    drop(first);
    next.shutdown(Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

//! End to end against a packaged engine: start the host, open a page, and
//! read the frame's pixels out of the shared surface.
//!
//! ```sh
//! COLOSSO_BROWSER_ENGINE=<data>/browser-engine/current \
//!   cargo test -p colosso-browser-host --test smoke -- --ignored
//! ```
#![cfg(target_os = "macos")]
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

mod support;

use colosso_browser_ipc::client::{Connection, Incoming};
use colosso_browser_ipc::{AppRequest, HostEvent, Viewport};
use objc2_io_surface::IOSurfaceRef;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use support::{Session, centre, is_red, launch};

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn dev_tools_paint_into_surfaces_of_their_own_and_close_with_their_page() {
    let launch = launch("devtools");
    let mut session = Session::start(&launch);
    let viewport = Viewport {
        width: 400,
        height: 300,
        scale: 1.,
    };
    let page = "data:text/html,<title>Inspected</title><body style='margin:0;background:rgb(255,0,0)'><p>hi</p></body>";
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: page.into(),
            viewport,
            profile: "smoke-devtools".into(),
        })
        .expect("open");
    session.wait("the page's first red frame", |event, seen| {
        matches!(event, HostEvent::Frame { pane: 1, .. }) && seen.is_some_and(|s| is_red(s.centre))
    });

    session
        .host
        .send(&AppRequest::OpenDevTools {
            pane: 1,
            devtools: 2,
            viewport,
            inspect: Some(colosso_browser_ipc::Point { x: 4., y: 4. }),
        })
        .expect("open devtools");
    session.wait("the DevTools surfaces", |event, _| {
        matches!(event, HostEvent::Surfaces { pane: 2, .. })
    });
    session.wait("the DevTools page", |event, _| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.url.starts_with("devtools://"))
    });
    session.wait(
        "a frame of the inspector, which is not the page",
        |event, seen| {
            matches!(event, HostEvent::Frame { pane: 2, .. })
                && seen.is_some_and(|s| !is_red(s.centre))
        },
    );
    // The frontend names the page it is attached to once the session is up.
    session.wait("the inspector to attach to the page", |event, _| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.title.starts_with("DevTools - data:text/html"))
    });
    // Opening again does not make a second inspector.
    session
        .host
        .send(&AppRequest::OpenDevTools {
            pane: 1,
            devtools: 3,
            viewport,
            inspect: None,
        })
        .expect("open devtools again");
    session.wait("the second inspector to be refused", |event, _| {
        matches!(event, HostEvent::Error { pane: Some(3), .. })
    });

    // Closed and opened again at once, as a toggle does, the page takes the
    // new inspector without waiting for the old one to be gone.
    session
        .host
        .send(&AppRequest::Close { pane: 2 })
        .expect("close devtools");
    session
        .host
        .send(&AppRequest::OpenDevTools {
            pane: 1,
            devtools: 4,
            viewport,
            inspect: None,
        })
        .expect("open devtools after closing them");
    session.wait("the new inspector to attach to the page", |event, _| {
        matches!(event, HostEvent::Page { pane: 4, state } if state.title.starts_with("DevTools - data:text/html"))
    });

    // The page closes; its DevTools go with it, and both are reported.
    session
        .host
        .send(&AppRequest::Close { pane: 1 })
        .expect("close");
    let mut closed = Vec::new();
    session.wait("the page and its inspector to close", |event, _| {
        if let HostEvent::Closed { pane } = event {
            closed.push(*pane);
        }
        closed.contains(&1) && closed.contains(&4)
    });
    session.host.shutdown(Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_paints_into_the_shared_surface() {
    let launch = launch("page");
    let (sink, incoming) = std::sync::mpsc::channel();
    let mut host = Connection::start(&launch, move |message| {
        sink.send(message).err().map(|e| e.0)
    })
    .expect("the host starts and says hello");
    assert!(host.chromium.starts_with("156."), "{}", host.chromium);
    let page = "data:text/html,<body style='margin:0;background:rgb(255,0,0)'></body>";
    host.send(&AppRequest::Open {
        pane: 1,
        url: page.into(),
        viewport: Viewport {
            width: 320,
            height: 200,
            scale: 2.,
        },
        profile: "smoke".into(),
    })
    .expect("open");

    let mut rings: HashMap<u32, Vec<objc2_core_foundation::CFRetained<IOSurfaceRef>>> =
        HashMap::new();
    let mut announced = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut red = None;
    while Instant::now() < deadline && red.is_none() {
        let Ok(incoming) = incoming.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };
        match incoming {
            Incoming::Ring(ring) => {
                let surfaces = ring
                    .ports
                    .iter()
                    .map(|port| {
                        let surface =
                            IOSurfaceRef::lookup_from_mach_port(*port).expect("surface from port");
                        colosso_browser_ipc::mach::release(*port);
                        surface
                    })
                    .collect();
                rings.insert(ring.generation, surfaces);
            }
            Incoming::Event(HostEvent::Surfaces {
                generation, size, ..
            }) => {
                assert_eq!(
                    (size.width, size.height),
                    (640, 400),
                    "device pixels at scale 2"
                );
                announced = Some(generation);
            }
            Incoming::Event(HostEvent::Frame {
                generation, slot, ..
            }) => {
                let surface = rings
                    .get(&generation)
                    .and_then(|r| r.get(usize::from(slot)))
                    .expect("the ring arrives before its frames are needed");
                let pixel = centre(surface);
                host.send(&AppRequest::Release {
                    pane: 1,
                    generation,
                    slot,
                })
                .expect("release");
                // BGRA: blue, green, red, alpha.
                if pixel[2] > 240 && pixel[1] < 16 && pixel[0] < 16 {
                    red = Some(pixel);
                }
            }
            Incoming::Event(HostEvent::Error { message, .. }) => panic!("host error: {message}"),
            Incoming::Event(_) => {}
            Incoming::Closed => panic!("the host went away"),
        }
    }
    assert!(announced.is_some(), "the ring was announced");
    assert!(red.is_some(), "a red frame arrived in the shared surface");
    let pid = host.pid();
    let quitting = Instant::now();
    host.shutdown(Duration::from_secs(10));
    let quit = quitting.elapsed();
    eprintln!("host {pid} quit in {quit:?}");
    assert!(
        quit < Duration::from_secs(5),
        "the host quits on request, not when killed"
    );
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_that_grows_or_shrinks_never_shows_a_letterboxed_frame() {
    let launch = launch("resize");
    let mut session = Session::start(&launch);
    let at = |height| Viewport {
        width: 600,
        height,
        scale: 2.,
    };
    let page = "data:text/html,<body style='margin:0;background:rgb(255,0,0)'></body>";
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: page.into(),
            viewport: at(300),
            profile: "smoke-resize".into(),
        })
        .expect("open");
    session.wait("the page's first red frame", |event, seen| {
        matches!(event, HostEvent::Frame { .. }) && seen.is_some_and(|s| is_red(s.centre))
    });
    // Grow past the ring, as when docked DevTools close, then shrink within
    // it, as when they open: every frame is the page edge to edge, never the
    // new layout scaled into the old size between black bars.
    for height in [700, 400] {
        session
            .host
            .send(&AppRequest::Resize {
                pane: 1,
                viewport: at(height),
            })
            .expect("resize");
        let want = at(height).pixels();
        session.wait("a frame at the new size", |event, seen| {
            let HostEvent::Frame { size, .. } = event else {
                return false;
            };
            let seen = seen.expect("frames are read");
            assert!(
                seen.grid.iter().all(|p| is_red(*p)),
                "a {size:?} frame after resizing to {want:?} is not the page edge to edge"
            );
            *size == want
        });
    }
    session.host.shutdown(Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn agents_run_protocol_commands_on_the_page_beside_its_dev_tools() {
    let launch = launch("agent");
    let mut session = Session::start(&launch);
    let viewport = Viewport {
        width: 400,
        height: 300,
        scale: 1.,
    };
    let page = "data:text/html,<title>Agent</title><script>console.log('before-the-agent')</script>\
        <body style='margin:0;background:rgb(255,0,0)'><button>Go</button></body>";
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: page.into(),
            viewport,
            profile: "smoke-agent".into(),
        })
        .expect("open");
    // A command sent before the page exists waits for it.
    cdp(
        &mut session,
        1,
        "Runtime.evaluate",
        serde_json::json!({"expression": "document.title", "returnByValue": true}),
    );
    let (result, error) = reply(&mut session, 1);
    assert_eq!(error, None);
    assert_eq!(result.expect("a result")["result"]["value"], "Agent");

    // DevTools open beside the agent; each keeps its own answers.
    session
        .host
        .send(&AppRequest::OpenDevTools {
            pane: 1,
            devtools: 2,
            viewport,
            inspect: None,
        })
        .expect("open devtools");
    session.wait("the inspector to attach to the page", |event, _| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.title.starts_with("DevTools - "))
    });
    cdp(&mut session, 2, "No.such", serde_json::json!({}));
    cdp(&mut session, 3, "Runtime.enable", serde_json::Value::Null);
    let (mut failed, mut enabled, mut logged) = (None, false, false);
    session.wait(
        "the bad command, Runtime.enable and the earlier log",
        |event, _| {
            match event {
                HostEvent::CdpReply { call: 2, error, .. } => failed = error.clone(),
                HostEvent::CdpReply {
                    call: 3,
                    error: None,
                    ..
                } => enabled = true,
                HostEvent::CdpEvent { method, params, .. }
                    if method == "Runtime.consoleAPICalled" =>
                {
                    logged |= params["args"][0]["value"] == "before-the-agent";
                }
                _ => {}
            }
            failed.is_some() && enabled && logged
        },
    );
    assert!(failed.expect("an error").contains("wasn't found"));

    // The page closes with a command under way: it is answered all the same.
    cdp(
        &mut session,
        4,
        "Runtime.evaluate",
        serde_json::json!({"expression": "new Promise(() => {})", "awaitPromise": true}),
    );
    session
        .host
        .send(&AppRequest::Close { pane: 1 })
        .expect("close");
    let (_, error) = reply(&mut session, 4);
    assert!(error.is_some());
    session.host.shutdown(Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

/// Sends the agent command `call` on pane 1.
fn cdp(session: &mut Session, call: u64, method: &str, params: serde_json::Value) {
    session
        .host
        .send(&AppRequest::Cdp {
            pane: 1,
            call,
            method: method.into(),
            params,
        })
        .expect("cdp");
}

/// The host's reply to the agent command `call`: its result, or its error.
fn reply(session: &mut Session, call: u64) -> (Option<serde_json::Value>, Option<String>) {
    let mut answer = None;
    session.wait("the agent's answer", |event, _| {
        if let HostEvent::CdpReply {
            call: answered,
            result,
            error,
            ..
        } = event
            && *answered == call
        {
            answer = Some((result.clone(), error.clone()));
            return true;
        }
        false
    });
    answer.expect("answered")
}

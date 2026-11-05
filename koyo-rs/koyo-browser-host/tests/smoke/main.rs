//! End to end against a packaged engine: start the host, open a page, and
//! read the frame's pixels out of the shared surface.
//!
//! ```sh
//! KOYO_BROWSER_ENGINE=<data>/browser-engine/current \
//!   script/cargo test -p koyo-browser-host --test smoke -- --ignored
//! ```
#![cfg(target_os = "macos")]
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

mod agents;
mod input;
mod live_resize;
mod location;
mod support;

use agents::{cdp, reply};
use koyo_browser_ipc::client::{Connection, Incoming};
use koyo_browser_ipc::{AppRequest, HostEvent, PinchPhase, Viewport};
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
            inspect: Some(koyo_browser_ipc::Point { x: 4., y: 4. }),
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
                        koyo_browser_ipc::mach::release(*port);
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
                dragging: false,
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
fn a_trackpad_pinch_zooms_the_page_around_it_and_back_out() {
    let launch = launch("pinch");
    let mut session = Session::start(&launch);
    let viewport = Viewport {
        width: 400,
        height: 300,
        scale: 2.,
    };
    let page = "data:text/html,<body style='margin:0;width:3000px;height:3000px'>";
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: page.into(),
            viewport,
            profile: "smoke-pinch".into(),
        })
        .expect("open");
    let unzoomed = |[scale, left, top]: [f64; 3]| {
        (scale - 1.).abs() < 1e-3 && left.abs() < 1. && top.abs() < 1.
    };
    assert!(unzoomed(zoom(&mut session, |_| true)));
    pinch(&mut session, 0.1, 10);
    let [scale, left, top] = zoom(&mut session, |[scale, ..]| scale > 2.);
    // The point under the pinch stays where it was.
    assert!(
        (left - 300. * (1. - 1. / scale)).abs() < 10.,
        "{scale} {left}"
    );
    assert!(
        (top - 200. * (1. - 1. / scale)).abs() < 10.,
        "{scale} {top}"
    );
    pinch(&mut session, -0.1, 30);
    assert!(unzoomed(zoom(&mut session, unzoomed)));
    session.host.shutdown(Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

/// A whole pinch on pane 1 around (300, 200): `steps` events of `delta` each, each played
/// on its own as a trackpad's come, the host answering in between.
fn pinch(session: &mut Session, delta: f32, steps: usize) {
    let phases = std::iter::once((PinchPhase::Started, 0.))
        .chain(std::iter::repeat_n((PinchPhase::Moved, delta), steps))
        .chain([(PinchPhase::Ended, 0.)]);
    for (call, (phase, delta)) in (2000..).zip(phases) {
        session
            .host
            .send(&AppRequest::Pinch {
                pane: 1,
                x: 300.,
                y: 200.,
                delta,
                phase,
            })
            .expect("pinch");
        cdp(
            session,
            call,
            "Runtime.evaluate",
            serde_json::json!({"expression": "0"}),
        );
        reply(session, call);
    }
}

/// Pane 1's visual viewport, `[scale, left, top]`, once `settled` holds of it: asked again
/// each time the page is drawn anew.
fn zoom(session: &mut Session, settled: impl Fn([f64; 3]) -> bool) -> [f64; 3] {
    let expression = "[visualViewport.scale, visualViewport.offsetLeft, visualViewport.offsetTop]";
    let mut call = 1000;
    loop {
        call += 1;
        let params = serde_json::json!({"expression": expression, "returnByValue": true});
        cdp(session, call, "Runtime.evaluate", params);
        let (result, error) = reply(session, call);
        assert_eq!(error, None);
        let value = result.expect("a result")["result"]["value"].clone();
        let zoom = serde_json::from_value(value).expect("three numbers");
        if settled(zoom) {
            return zoom;
        }
        session.wait("the page drawn anew", |event, _| {
            matches!(event, HostEvent::Frame { .. })
        });
    }
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_host_left_holding_the_profiles_gives_way_to_the_next() {
    let launch = launch("left");
    let first = Session::start(&launch);
    let stuck = first.host.pid();
    // Stopped, it answers no one, as a host whose app died mid-shutdown may not: Chromium
    // would make the next host wait 20 seconds for it, past the hello.
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
}

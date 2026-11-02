//! A page dragged wider, as a pane's divider or a responsive grip does: a new layout in every
//! frame while the drag goes on, and the page edge to edge once it stops.

use colosso_browser_ipc::{AppRequest, HostEvent, Viewport};
use std::time::{Duration, Instant};

use crate::support::{Session, is_red, launch};

/// A page whose red is exactly its layout's width, on white.
const VW_PAGE: &str = "data:text/html,<html style='background:white'><body style='margin:0'><div style='width:100vw;height:100vh;background:rgb(255,0,0)'></div></body></html>";

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_dragged_wider_lays_out_again_every_frame() {
    let launch = launch("live-resize");
    let mut session = Session::start(&launch);
    let at = |width| Viewport {
        width,
        height: 400,
        scale: 2.,
    };
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: VW_PAGE.into(),
            viewport: at(600),
            profile: "smoke-live-resize".into(),
        })
        .expect("open");
    session.wait("the page's first red frame", |event, seen| {
        matches!(event, HostEvent::Frame { .. }) && seen.is_some_and(|s| is_red(s.grid[0]))
    });
    // Loaded: live sizing needs the page's session.
    session.pump(Instant::now() + Duration::from_millis(500), |_, _| false);
    // 120 steps of 4 px, one per 60 Hz frame: two seconds of drag.
    let start = Instant::now();
    let mut frames: Vec<(u128, u32, usize, u32)> = Vec::new();
    for step in 0..120u32 {
        let width = 600 + 4 * step;
        session
            .host
            .send(&AppRequest::Resize {
                pane: 1,
                viewport: at(width),
            })
            .expect("resize");
        let next = start + Duration::from_micros(16_667 * u64::from(step + 1));
        session.pump(next, |event, seen| {
            if let (HostEvent::Frame { size, .. }, Some(seen)) = (event, seen) {
                frames.push((
                    start.elapsed().as_millis(),
                    size.width / 2,
                    red_edge(seen),
                    width,
                ));
            }
            false
        });
    }
    let last = 600 + 4 * 119;
    let mut settled = None;
    session.pump(Instant::now() + Duration::from_secs(2), |event, seen| {
        if let (HostEvent::Frame { size, .. }, Some(seen)) = (event, seen) {
            settled = Some((
                size.width / 2,
                red_edge(seen),
                seen.grid.iter().all(|p| is_red(*p)),
            ));
        }
        false
    });
    let mut layouts: Vec<usize> = frames.iter().map(|f| f.2).collect();
    layouts.dedup();
    let behind: Vec<i64> = frames
        .iter()
        .map(|(_, _, edge, asked)| i64::from(*asked) - i64::try_from(*edge).unwrap_or(0))
        .collect();
    eprintln!(
        "drag: {} frames, {} layouts; behind the drag by (px): {:?}",
        frames.len(),
        layouts.len(),
        behind.iter().take(60).collect::<Vec<_>>()
    );
    eprintln!(
        "first frames (ms, frame width, red edge, asked): {:?}",
        &frames[..frames.len().min(12)]
    );
    eprintln!("settled: {settled:?}");
    session.host.shutdown(Duration::from_secs(10));
    assert!(
        frames.len() >= 100,
        "a frame per display frame, not {}",
        frames.len()
    );
    assert!(
        layouts.len() >= 100,
        "a new layout per frame, not {}",
        layouts.len()
    );
    let (width, _, whole) = settled.expect("frames after the drag");
    assert_eq!(width, last, "the view ends at the drag's last size");
    assert!(
        whole,
        "the page fills its frame edge to edge once the drag is over"
    );
}

/// Where the red page ends along a frame's middle row, in logical px at scale 2.
fn red_edge(seen: &crate::support::Seen) -> usize {
    seen.row.iter().take_while(|p| is_red(**p)).count() * 2
}

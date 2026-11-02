use super::*;

fn at(width: u32, height: u32) -> Viewport {
    Viewport {
        width,
        height,
        scale: 2.,
    }
}

/// The view pinned at the drag's start grows to its room and paints there: the room.
fn grow(live: &mut Live) -> Viewport {
    let Phase::Pinning { room } = live.phase else {
        panic!("not pinning: {:?}", live.phase);
    };
    assert_eq!(live.pinned(), vec![Do::Resize(room)]);
    live.painted(room.pixels());
    room
}

#[test]
fn a_drag_grows_the_view_at_once_under_the_layout_whichever_way_it_goes() {
    let start = Instant::now();
    let (mut live, todo) = Live::start(at(1000, 600), at(996, 600), None, start);
    let room = at(1512, 1112);
    assert_eq!(todo, vec![Do::Pin(at(996, 600))], "laid out, then grown");
    assert_eq!(live.phase, Phase::Pinning { room });
    assert_eq!(
        live.ask(at(990, 600), start + Duration::from_millis(16)),
        vec![Do::Override(at(990, 600))]
    );
    assert_eq!(
        live.laid().pixels(),
        at(990, 600).pixels(),
        "frames are cut to it"
    );
    assert_eq!(live.pinned(), vec![Do::Resize(room)]);
    assert_eq!(
        live.painted(at(1000, 600).pixels()),
        vec![],
        "not grown yet"
    );
    assert_eq!(
        live.painted(room.pixels()),
        vec![Do::Override(at(990, 600))],
        "grown: the latest size"
    );
}

#[test]
fn a_drag_past_the_view_is_capped_until_the_view_has_grown() {
    let start = Instant::now();
    let (mut live, todo) = Live::start(at(1000, 600), at(1004, 600), None, start);
    let room = at(1516, 1112);
    assert_eq!(todo, vec![Do::Pin(at(1000, 600))]);
    assert_eq!(live.phase, Phase::Pinning { room });
    assert_eq!(
        live.ask(at(1008, 600), start),
        vec![Do::Override(at(1000, 600))],
        "still capped while pinning"
    );
    assert_eq!(grow(&mut live), room);
    assert_eq!(
        live.ask(at(1400, 900), start),
        vec![Do::Override(at(1400, 900))]
    );
}

#[test]
fn a_drag_past_the_room_grows_it_again() {
    let start = Instant::now();
    let (mut live, _) = Live::start(at(1000, 600), at(996, 600), None, start);
    let room = grow(&mut live);
    let wider = at(2100, 600);
    assert_eq!(
        live.ask(wider, start),
        vec![
            Do::Override(at(room.width, 600)),
            Do::Resize(at(2612, 1624))
        ]
    );
    assert_eq!(
        live.painted(at(2612, 1624).pixels()),
        vec![Do::Override(wider)]
    );
}

#[test]
fn the_room_reaches_the_largest_display_so_a_fast_drag_grows_the_view_once() {
    let start = Instant::now();
    let (mut live, todo) = Live::start(at(800, 600), at(900, 600), Some((1920, 1200)), start);
    let room = at(1920, 1200);
    assert_eq!(todo, vec![Do::Pin(at(800, 600))]);
    assert_eq!(live.phase, Phase::Pinning { room });
    grow(&mut live);
    assert_eq!(
        live.ask(at(1700, 900), start),
        vec![Do::Override(at(1700, 900))],
        "within the display: laid out, the view as it is"
    );
}

#[test]
fn a_drag_ends_with_the_view_at_its_last_size_and_the_override_gone() {
    let start = Instant::now();
    let (mut live, _) = Live::start(at(1000, 600), at(900, 600), None, start);
    let room = grow(&mut live);
    assert_eq!(
        live.settle(start + Duration::from_millis(100)),
        Err(Duration::from_millis(50))
    );
    let quiet = start + SETTLE;
    assert_eq!(live.settle(quiet), Ok(vec![Do::Resize(at(900, 600))]));
    assert_eq!(live.painted(room.pixels()), vec![], "still the grown view");
    assert!(!live.done());
    assert_eq!(live.painted(at(900, 600).pixels()), vec![Do::Clear]);
    assert!(live.done());
    assert_eq!(live.laid().pixels(), at(900, 600).pixels());
}

#[test]
fn a_drag_on_a_view_that_cant_grow_ends_at_once_back_at_its_size() {
    let start = Instant::now();
    let most = at(8192, 8192);
    let (mut live, todo) = Live::start(most, at(8000, 8192), Some((8192, 8192)), start);
    assert_eq!(todo, vec![Do::Override(at(8000, 8192))], "no room to grow");
    live.ask(most, start);
    assert_eq!(live.settle(start + SETTLE), Ok(vec![Do::Clear]));
    assert!(live.done());
}

#[test]
fn an_end_that_never_paints_gives_the_page_its_metrics_back_anyway() {
    let start = Instant::now();
    let (mut live, _) = Live::start(at(1000, 600), at(900, 600), None, start);
    grow(&mut live);
    live.settle(start + SETTLE).ok();
    assert!(
        live.settle(start + SETTLE + Duration::from_millis(100))
            .is_err()
    );
    assert_eq!(
        live.settle(start + SETTLE + PAINT_WAIT),
        Ok(vec![Do::Clear])
    );
}

#[test]
fn a_size_while_the_drag_ends_takes_it_up_again() {
    let start = Instant::now();
    let (mut live, _) = Live::start(at(1000, 600), at(900, 600), None, start);
    grow(&mut live);
    live.settle(start + SETTLE).ok();
    let later = start + SETTLE + Duration::from_millis(20);
    assert_eq!(
        live.ask(at(880, 600), later),
        vec![Do::Override(at(880, 600))]
    );
    assert_eq!(
        live.ask(at(950, 600), later),
        vec![Do::Override(at(900, 600)), Do::Resize(at(1462, 1112))],
        "the view is 900 wide now: past it, it grows"
    );
}

#[test]
fn the_room_stays_within_the_largest_surface() {
    let start = Instant::now();
    let (live, todo) = Live::start(at(8000, 600), at(8100, 600), None, start);
    assert_eq!(todo, vec![Do::Pin(at(8000, 600))]);
    let Phase::Pinning { room } = live.phase else {
        panic!("not pinning: {:?}", live.phase);
    };
    assert_eq!(room.width, 8192, "16384 device px at 2x");
}

#[test]
fn the_override_keeps_the_apps_own_metrics() {
    let own = serde_json::json!({"width": 0, "height": 0, "deviceScaleFactor": 0, "mobile": true});
    assert_eq!(
        metrics(Some(&own), at(393, 852)),
        serde_json::json!({"width": 393, "height": 852, "deviceScaleFactor": 0, "mobile": true})
    );
    assert_eq!(
        metrics(None, at(800, 600)),
        serde_json::json!({"width": 800, "height": 600, "deviceScaleFactor": 0, "mobile": false})
    );
}

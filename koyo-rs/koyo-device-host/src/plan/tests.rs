//! Acts become the touches the proof of concept measured.

use super::*;
use koyo_device_wire::Spot;

const PHONE: Size = Size {
    width: 402.,
    height: 874.,
};

fn at(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn steps(act: &Act) -> Vec<Timed> {
    plan(act, at(201., 437.), at(201., 200.), PHONE).unwrap()
}

#[test]
fn a_tap_is_down_and_up_and_a_double_tap_twice_90_ms_apart() {
    let tap = Act::Tap {
        spot: Spot::Point(at(201., 437.)),
        count: 2,
        hold_ms: 0,
        fingers: 1,
    };
    let planned = steps(&tap);
    let times: Vec<u32> = planned.iter().map(|t| t.at_ms).collect();
    assert_eq!(times, [0, 60, 150, 210]);
    assert!(
        matches!(&planned[0].touch, Touch::Fingers { touching: true, at, .. } if at == &[at_(201., 437.)])
    );
    assert!(matches!(
        &planned[1].touch,
        Touch::Fingers {
            touching: false,
            ..
        }
    ));
}

fn at_(x: f64, y: f64) -> Point {
    at(x, y)
}

#[test]
fn a_long_press_is_held_as_long_as_asked() {
    let hold = Act::Tap {
        spot: Spot::Point(at(201., 437.)),
        count: 1,
        hold_ms: 900,
        fingers: 1,
    };
    let times: Vec<u32> = steps(&hold).iter().map(|t| t.at_ms).collect();
    assert_eq!(times, [0, 900]);
}

#[test]
fn three_fingers_tap_side_by_side() {
    let tap = Act::Tap {
        spot: Spot::Point(at(201., 437.)),
        count: 1,
        hold_ms: 0,
        fingers: 3,
    };
    let Touch::Fingers { at, .. } = &steps(&tap)[0].touch else {
        panic!("fingers");
    };
    let xs: Vec<f64> = at.iter().map(|p| p.x).collect();
    assert_eq!(xs, [171., 201., 231.]);
}

#[test]
fn a_swipe_goes_at_sixty_steps_a_second_and_lifts_where_it_ended() {
    let swipe = Act::Swipe {
        from: spot_from(),
        to: Spot::Point(at(201., 200.)),
        ms: 160,
        fingers: 1,
    };
    let timed = steps(&swipe);
    // Down, ten steps, the last place once more, then up a frame and more after it.
    assert_eq!(timed.len(), 13);
    assert_eq!(timed[10].at_ms, 160);
    assert_eq!(timed[11].at_ms, 176);
    assert_eq!(timed[12].at_ms, 201);
    let Touch::Fingers {
        at: again,
        touching: still,
        ..
    } = &timed[11].touch
    else {
        panic!("fingers");
    };
    assert!(still);
    assert_eq!(again, &[at_(201., 200.)]);
    let Touch::Fingers { at, touching, .. } = &timed[12].touch else {
        panic!("fingers");
    };
    assert!(!touching);
    assert_eq!(at, &[at_(201., 200.)]);
}

fn spot_from() -> Spot {
    Spot::Point(at(201., 437.))
}

#[test]
fn the_app_switcher_is_a_slow_swipe_from_the_bottom_edge_held_a_second() {
    let timed = steps(&Act::Button {
        button: Press::AppSwitcher,
    });
    let Touch::Fingers { at, edge, .. } = &timed[0].touch else {
        panic!("fingers");
    };
    assert_eq!(*edge, Some(Edge::Bottom));
    assert!((at[0].y - 0.995 * PHONE.height).abs() < 1e-9);
    let last = timed.last().unwrap().at_ms;
    assert!((1_600..1_700).contains(&last), "{last}");
}

#[test]
fn a_swipe_from_the_very_edge_carries_its_flag() {
    let from_left = plan(
        &Act::Swipe {
            from: Spot::Point(at(1., 437.)),
            to: Spot::Point(at(300., 437.)),
            ms: 300,
            fingers: 1,
        },
        at(1., 437.),
        at(300., 437.),
        PHONE,
    )
    .unwrap();
    assert!(matches!(
        &from_left[0].touch,
        Touch::Fingers {
            edge: Some(Edge::Left),
            ..
        }
    ));
}

#[test]
fn a_pinch_spreads_two_fingers_to_its_scale() {
    let timed = steps(&Act::Pinch {
        spot: Spot::Point(at(201., 437.)),
        scale: 2.,
        degrees: 90.,
        ms: 320,
    });
    let spread = |t: &Timed| {
        let Touch::Fingers { at, .. } = &t.touch else {
            panic!("fingers");
        };
        (at[1].x - at[0].x).hypot(at[1].y - at[0].y)
    };
    let (first, last) = (spread(&timed[0]), spread(timed.last().unwrap()));
    assert!((last / first - 2.).abs() < 1e-9, "{first} {last}");
    let Touch::Fingers { at, .. } = &timed.last().unwrap().touch else {
        panic!("fingers");
    };
    // Turned a quarter: one above the other.
    assert!((at[0].x - at[1].x).abs() < 1e-9);
}

#[test]
fn a_pinch_at_the_edge_keeps_its_fingers_on_the_screen() {
    let timed = plan(
        &Act::Pinch {
            spot: Spot::Point(at(5., 400.)),
            scale: 2.,
            degrees: 0.,
            ms: 320,
        },
        at(5., 400.),
        at(5., 400.),
        PHONE,
    )
    .unwrap();
    for step in &timed {
        if let Touch::Fingers { at, .. } = &step.touch {
            assert!(at.iter().all(|p| (1. ..=401.).contains(&p.x)), "{at:?}");
        }
    }
}

#[test]
fn text_is_typed_as_characters_with_shift_round_those_that_need_it() {
    let timed = typed("aB!").unwrap();
    let keys: Vec<(u16, bool)> = timed
        .iter()
        .map(|t| match t.touch {
            Touch::Key { usage, down } => (usage, down),
            _ => panic!("keys"),
        })
        .collect();
    assert_eq!(
        keys,
        [
            (4, true),
            (4, false),
            (0xE1, true),
            (5, true),
            (5, false),
            (0xE1, false),
            (0xE1, true),
            (30, true),
            (30, false),
            (0xE1, false)
        ]
    );
    let refused = typed("café ☕").unwrap_err();
    assert_eq!(refused.code, RefusalCode::Invalid);
    assert!(refused.message.contains("é"), "{}", refused.message);
}

#[test]
fn a_combo_presses_its_modifiers_round_the_key() {
    let timed = combo_keys("cmd+space").unwrap();
    let keys: Vec<(u16, bool)> = timed
        .iter()
        .map(|t| match t.touch {
            Touch::Key { usage, down } => (usage, down),
            _ => panic!("keys"),
        })
        .collect();
    assert_eq!(keys, [(0xE3, true), (44, true), (44, false), (0xE3, false)]);
    assert!(combo_keys("cmd+nonsense").is_err());
    assert!(combo_keys("cmd+").is_err());
}

#[test]
fn a_place_off_the_screen_is_refused() {
    let tap = Act::Tap {
        spot: Spot::Point(at(500., 437.)),
        count: 1,
        hold_ms: 0,
        fingers: 1,
    };
    let refused = plan(&tap, at(500., 437.), at(0., 0.), PHONE).unwrap_err();
    assert_eq!(refused.code, RefusalCode::Invalid);
}

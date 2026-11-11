//! The proof of concept's fourteen tests of the mapping, on the link's input. A step is written
//! as the proof of concept's lines were, in shares of the screen: `d` a finger touching, `u`
//! lifting, `2` for two fingers, then the edge's flag for one.

use super::*;

/// An iPhone 17 Pro's screen in points.
const SIZE: Size = Size {
    width: 402.,
    height: 874.,
};

fn fingers() -> Fingers {
    Fingers::new(SIZE)
}

fn line(step: &Step) -> String {
    let Input::Touches { fingers, edge } = &step.input else {
        return format!("{:?}", step.input);
    };
    let share = |f: &Finger| (f.at.x / SIZE.width, f.at.y / SIZE.height);
    let verb = if fingers[0].down { "d" } else { "u" };
    match fingers.as_slice() {
        [one] => {
            let (x, y) = share(one);
            format!("{verb} {x:.4} {y:.4} {}", edge.map_or(0, Edge::flag))
        }
        [first, second] => {
            let ((x1, y1), (x2, y2)) = (share(first), share(second));
            format!("{verb}2 {x1:.4} {y1:.4} {x2:.4} {y2:.4}")
        }
        _ => format!("{fingers:?}"),
    }
}

fn lines(steps: &[Step]) -> Vec<String> {
    steps.iter().map(line).collect()
}

fn numbers(line: &str) -> Vec<f32> {
    line.split_whitespace()
        .skip(1)
        .map(|n| n.parse().expect("a number"))
        .collect()
}

/// How far apart a two-finger line's fingers are, in pixels, and the line they're on.
fn span(line: &str) -> (f32, f32) {
    let n = numbers(line);
    let (dx, dy) = ((n[2] - n[0]) * 1206., (n[3] - n[1]) * 2622.);
    (dx.hypot(dy), dy.atan2(dx).to_degrees())
}

fn press(x: f32, y: f32) -> Mac {
    Mac::Press {
        at: At::new(x, y),
        option: false,
    }
}

fn drag(x: f32, y: f32) -> Mac {
    Mac::Drag {
        at: At::new(x, y),
        shift: false,
    }
}

fn release(x: f32, y: f32) -> Mac {
    Mac::Release { at: At::new(x, y) }
}

fn scroll(phase: Phase, dy: f32, precise: bool) -> Mac {
    Mac::Scroll {
        at: At::new(0.5, 0.5),
        phase,
        dx: 0.,
        dy,
        precise,
    }
}

fn pinch(phase: Phase, delta: f32) -> Mac {
    Mac::Pinch {
        at: At::new(0.5, 0.5),
        phase,
        delta,
    }
}

fn rotate(phase: Phase, degrees: f32) -> Mac {
    Mac::Rotate {
        at: At::new(0.5, 0.5),
        phase,
        degrees,
    }
}

#[test]
fn a_click_is_a_finger_down_and_up_where_it_was() {
    let mut f = fingers();
    assert_eq!(lines(&f.take(press(0.5, 0.25), 0)), ["d 0.5000 0.2500 0"]);
    assert!(f.down());
    assert_eq!(lines(&f.take(drag(0.5, 0.5), 10)), ["d 0.5000 0.5000 0"]);
    assert_eq!(lines(&f.take(release(0.5, 0.5), 20)), ["u 0.5000 0.5000 0"]);
    assert!(!f.down());
}

#[test]
fn a_press_at_an_edge_keeps_that_edge_until_it_lifts() {
    let mut f = fingers();
    assert_eq!(lines(&f.take(press(0.5, 0.99), 0)), ["d 0.5000 0.9900 3"]);
    assert_eq!(lines(&f.take(drag(0.5, 0.5), 10)), ["d 0.5000 0.5000 3"]);
    assert_eq!(lines(&f.take(release(0.5, 0.5), 20)), ["u 0.5000 0.5000 3"]);
    assert_eq!(edge(At::new(0.5, 0.01)), Some(Edge::Top));
    assert_eq!(edge(At::new(0.01, 0.5)), Some(Edge::Left));
    assert_eq!(edge(At::new(0.99, 0.5)), Some(Edge::Right));
    assert_eq!(edge(At::new(0.5, 0.5)), None);
}

#[test]
fn a_drag_past_the_screen_stays_on_it() {
    let mut f = fingers();
    f.take(press(0.5, 0.5), 0);
    assert_eq!(lines(&f.take(drag(1.4, -0.2), 10)), ["d 1.0000 0.0000 0"]);
}

#[test]
fn a_trackpad_pinch_spreads_two_fingers_by_what_it_magnified() {
    let mut f = fingers();
    let down = lines(&f.take(pinch(Phase::Started, 0.), 0));
    assert_eq!(down.len(), 1);
    assert!(down[0].starts_with("d2 "));
    let (began, _) = span(&down[0]);
    let mut last = String::new();
    for step in 0..10 {
        let moved = lines(&f.take(pinch(Phase::Moved, 0.1), step));
        assert_eq!(moved.len(), 1);
        assert!(moved[0].starts_with("d2 "));
        last.clone_from(&moved[0]);
    }
    let (ended, line) = span(&last);
    // Ten steps of a tenth: 1.1 to the tenth.
    assert!(
        (ended / began - 1.1f32.powi(10)).abs() < 0.01,
        "{}",
        ended / began
    );
    assert!(line.abs() < 0.1);
    let up = lines(&f.take(pinch(Phase::Ended, 0.), 20));
    assert_eq!(up.len(), 1);
    assert!(up[0].starts_with("u2 "));
    assert!(!f.down());
}

#[test]
fn a_trackpad_rotation_turns_the_fingers_the_way_the_hand_turned() {
    let mut f = fingers();
    let down = lines(&f.take(rotate(Phase::Started, 0.), 0));
    let (began, _) = span(&down[0]);
    let mut last = String::new();
    for step in 0..9 {
        last = lines(&f.take(rotate(Phase::Moved, 5.), step)).remove(0);
    }
    let (ended, line) = span(&last);
    // Counter-clockwise for AppKit is a line that falls to the left on a screen.
    assert!((line + 45.).abs() < 0.2, "{line}");
    // Turning keeps the fingers as far apart, in pixels, whatever the screen's shape.
    assert!((ended - began).abs() < 1., "{began} {ended}");
    assert!(lines(&f.take(rotate(Phase::Ended, 0.), 20))[0].starts_with("u2 "));
}

#[test]
fn a_pinch_and_a_rotation_at_once_share_the_fingers_and_lift_them_once() {
    let mut f = fingers();
    assert_eq!(f.take(pinch(Phase::Started, 0.), 0).len(), 1);
    // The rotation joins: no second pair goes down.
    assert!(f.take(rotate(Phase::Started, 0.), 1).is_empty());
    assert!(lines(&f.take(pinch(Phase::Moved, 0.2), 2))[0].starts_with("d2"));
    assert!(lines(&f.take(rotate(Phase::Moved, 10.), 3))[0].starts_with("d2"));
    // One ends, the fingers stay for the other.
    assert!(f.take(pinch(Phase::Ended, 0.), 4).is_empty());
    assert!(f.down());
    assert!(lines(&f.take(rotate(Phase::Ended, 0.), 5))[0].starts_with("u2"));
    assert!(!f.down());
}

#[test]
fn a_trackpad_scroll_is_a_finger_that_drags_and_its_glide_is_left_to_the_device() {
    let mut f = fingers();
    // Fingers resting on the trackpad put none on the screen.
    assert!(f.take(scroll(Phase::Started, 0., true), 0).is_empty());
    assert_eq!(
        lines(&f.take(scroll(Phase::Moved, -0.1, true), 10)),
        ["d 0.5000 0.5000 0", "d 0.5000 0.4000 0"]
    );
    assert_eq!(
        lines(&f.take(scroll(Phase::Ended, 0., true), 20)),
        ["u 0.5000 0.4000 0"]
    );
    // The trackpad's own momentum comes after, with no beginning: nothing is touched.
    assert!(f.take(scroll(Phase::Moved, -0.05, true), 30).is_empty());
    assert!(!f.down());
}

#[test]
fn a_scroll_longer_than_the_screen_takes_hold_again_where_it_began() {
    let mut f = fingers();
    f.take(scroll(Phase::Started, 0., true), 0);
    f.take(scroll(Phase::Moved, -0.4, true), 10);
    let steps = lines(&f.take(scroll(Phase::Moved, -0.2, true), 20));
    assert_eq!(
        steps,
        [
            "u 0.5000 0.1000 0",
            "d 0.5000 0.5000 0",
            "d 0.5000 0.3000 0"
        ]
    );
}

#[test]
fn trackpad_fingers_that_just_stop_are_lifted_and_come_down_again_when_they_go_on() {
    let mut f = fingers();
    f.take(scroll(Phase::Started, 0., true), 0);
    f.take(scroll(Phase::Moved, -0.1, true), 10);
    assert!(f.tick(200).is_empty());
    assert_eq!(
        lines(&f.tick(10 + TRACKPAD_RESTS_MS)),
        ["u 0.5000 0.4000 0"]
    );
    // The same scroll goes on: a finger again, where the pointer is.
    assert_eq!(
        lines(&f.take(scroll(Phase::Moved, -0.1, true), 900)),
        ["d 0.5000 0.5000 0", "d 0.5000 0.4000 0"]
    );
    // A pinch the system took back never says it ended.
    let mut f = fingers();
    f.take(pinch(Phase::Started, 0.), 0);
    f.take(pinch(Phase::Moved, 0.1), 10);
    assert!(lines(&f.tick(10 + TRACKPAD_RESTS_MS))[0].starts_with("u2 "));
    assert!(!f.down());
}

#[test]
fn a_wheel_drags_a_finger_that_lifts_once_the_wheel_rests() {
    let mut f = fingers();
    assert_eq!(
        lines(&f.take(scroll(Phase::Moved, -0.04, false), 0)),
        ["d 0.5000 0.5000 0", "d 0.5000 0.4600 0"]
    );
    assert_eq!(
        lines(&f.take(scroll(Phase::Moved, -0.04, false), 50)),
        ["d 0.5000 0.4200 0"]
    );
    assert!(f.tick(100).is_empty());
    assert_eq!(lines(&f.tick(50 + WHEEL_RESTS_MS)), ["u 0.5000 0.4200 0"]);
    assert!(!f.down());
}

#[test]
fn option_drag_is_two_fingers_across_the_middle_and_shift_moves_them_together() {
    let mut f = fingers();
    let option = Mac::Press {
        at: At::new(0.4, 0.4),
        option: true,
    };
    assert_eq!(
        lines(&f.take(option, 0)),
        ["d2 0.4000 0.4000 0.6000 0.6000"]
    );
    // Towards the middle: a pinch in.
    assert_eq!(
        lines(&f.take(drag(0.45, 0.45), 10)),
        ["d2 0.4500 0.4500 0.5500 0.5500"]
    );
    // With Shift both go the same way.
    let together = Mac::Drag {
        at: At::new(0.45, 0.35),
        shift: true,
    };
    assert_eq!(
        lines(&f.take(together, 20)),
        ["d2 0.4500 0.3500 0.5500 0.4500"]
    );
    assert_eq!(
        lines(&f.take(release(0.45, 0.35), 30)),
        ["u2 0.4500 0.3500 0.5500 0.4500"]
    );
    assert!(!f.down());
}

#[test]
fn a_scroll_does_nothing_while_the_button_holds_a_finger_down() {
    let mut f = fingers();
    f.take(press(0.5, 0.5), 0);
    assert!(f.take(scroll(Phase::Started, 0.1, true), 5).is_empty());
    assert!(f.take(pinch(Phase::Started, 0.1), 6).is_empty());
    assert_eq!(lines(&f.take(release(0.5, 0.5), 10)), ["u 0.5000 0.5000 0"]);
}

#[test]
fn a_two_finger_double_tap_is_a_double_tap() {
    let zoom = Mac::SmartZoom {
        at: At::new(0.25, 0.75),
    };
    let steps = fingers().take(zoom, 0);
    assert_eq!(
        lines(&steps),
        [
            "d 0.2500 0.7500 0",
            "u 0.2500 0.7500 0",
            "d 0.2500 0.7500 0",
            "u 0.2500 0.7500 0"
        ]
    );
    let gaps: Vec<u32> = steps.iter().map(|s| s.after_ms).collect();
    assert_eq!(gaps, [0, 50, 140, 190]);
}

#[test]
fn keys_go_by_their_hid_usage_with_their_modifiers_round_them() {
    assert_eq!(key_usage("a"), Some(4));
    assert_eq!(key_usage("z"), Some(29));
    assert_eq!(key_usage("1"), Some(30));
    assert_eq!(key_usage("0"), Some(39));
    assert_eq!(key_usage("enter"), Some(40));
    assert_eq!(key_usage("left"), Some(80));
    assert_eq!(key_usage("f13"), None);
    let key = |usage, down| Input::Key { usage, down };
    assert_eq!(
        chord("a", true, false, false),
        [
            key(SHIFT, true),
            key(4, true),
            key(4, false),
            key(SHIFT, false)
        ]
    );
    assert_eq!(
        chord("enter", false, false, false),
        [key(40, true), key(40, false)]
    );
    assert!(chord("f13", false, false, false).is_empty());
}

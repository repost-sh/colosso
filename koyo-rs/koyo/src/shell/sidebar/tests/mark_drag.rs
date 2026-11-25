//! The spaces' marks dragged into a new order along the sidebar's foot (`docs/layout.md`,
//! Reordering spaces): the mark in hand follows the pointer and the others slide aside, and let
//! go its space goes to the slot it's over, the space shown staying shown and the order kept for
//! the next launch; pressed and let go without moving, a mark shows its space; pulled well off
//! the row, it slides back and nothing changes.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Pixels, Point, TestAppContext, VisualTestContext,
    point, px,
};

use super::space_keys::space;
use super::space_menu::id;
use super::space_switch::{at, in_spaces};
use crate::test_support::{click, draw, hold, let_go, move_held, shell_window};

/// The marks of the three spaces, by where they're listed.
const MARKS: [&str; 3] = ["space-mark-0", "space-mark-1", "space-mark-2"];

/// The spaces' ids in their order, and the place of the one shown among them.
fn spaces(cx: &TestAppContext) -> ([u64; 3], usize) {
    ([0, 1, 2].map(|place| id(place, cx)), space(cx))
}

/// Where each space's mark is drawn, in the order they're listed.
fn marks(window: AnyWindowHandle, cx: &mut TestAppContext) -> [Bounds<Pixels>; 3] {
    MARKS.map(|mark| {
        let drawn = VisualTestContext::from_window(window, cx).debug_bounds(mark);
        drawn.expect(mark)
    })
}

/// Whether two places along the foot are the same, but for rounding.
fn level(a: Pixels, b: Pixels) -> bool {
    (a.as_f32() - b.as_f32()).abs() < 0.5
}

/// Over the right of the last mark, past its middle: what's let go there goes last.
fn past_the_last(laid: &[Bounds<Pixels>; 3]) -> Point<Pixels> {
    laid[2].center() + point(px(6.), px(0.))
}

#[gpui_kit::test]
fn a_mark_dragged_along_the_foot_takes_its_space_there_and_the_space_shown_stays_shown(
    cx: &mut TestAppContext,
) {
    let (window, shell, [root]) = in_spaces("mark-drag-moves", (3, [("app", 0)]), cx);
    let (ids, shown) = spaces(cx);
    assert_eq!(shown, 0, "the first space is shown");
    let laid = marks(window, cx);
    let over = past_the_last(&laid);

    hold(window, laid[0].center(), cx);
    move_held(window, over, cx);
    draw(window, cx);
    let held = marks(window, cx);
    assert!(
        level(held[1].origin.x, laid[0].origin.x),
        "the next slid aside into the slot it left: {held:?}"
    );
    assert!(
        held[0].origin.x > held[2].origin.x,
        "the mark in hand is past the others, under the pointer: {held:?}"
    );
    assert_eq!(spaces(cx), (ids, 0), "nothing moves until it's let go");

    let_go(window, over, cx);
    let moved = [ids[1], ids[2], ids[0]];
    assert_eq!(spaces(cx), (moved, 2), "last, and still shown");
    assert_eq!(at(&shell, cx), (Some(root.clone()), 2), "its project still shown");
    let now = marks(window, cx);
    assert!(now[2].size.width > now[0].size.width, "the capsule went with it: {now:?}");

    drop(shell);
    (cx.update_window(window, |_, window, _| window.remove_window())).unwrap();
    cx.run_until_parked();
    let (window, shell) = shell_window(cx);
    draw(window, cx);
    assert_eq!(spaces(cx), (moved, 2), "the next launch keeps the order");
    assert_eq!(at(&shell, cx), (Some(root), 2), "and shows the space it showed");
    let now = marks(window, cx);
    assert!(now[2].size.width > now[0].size.width, "its capsule last: {now:?}");
}

#[gpui_kit::test]
fn a_mark_pressed_and_let_go_without_moving_shows_its_space(cx: &mut TestAppContext) {
    let (window, _shell, _) = in_spaces("mark-drag-click", (3, [("app", 0)]), cx);
    let (ids, _) = spaces(cx);
    click(window, MARKS[1], cx);
    assert_eq!(spaces(cx), (ids, 1), "shown, the order as it was");
}

#[gpui_kit::test]
fn a_mark_dragged_well_off_the_foot_slides_back_and_nothing_changes(cx: &mut TestAppContext) {
    let (window, _shell, _) = in_spaces("mark-drag-away", (3, [("app", 0)]), cx);
    let (ids, _) = spaces(cx);
    let laid = marks(window, cx);
    let over = past_the_last(&laid);

    hold(window, laid[1].center(), cx);
    move_held(window, over, cx);
    draw(window, cx);
    let held = marks(window, cx);
    assert!(
        level(held[2].origin.x, laid[1].origin.x),
        "the last slid aside into the slot it left: {held:?}"
    );

    let away = over - point(px(0.), px(80.));
    move_held(window, away, cx);
    draw(window, cx);
    let back = marks(window, cx);
    let in_place = (back.iter().zip(&laid)).all(|(now, was)| level(now.origin.x, was.origin.x));
    assert!(in_place, "every mark back in its slot while it's held: {back:?}");

    let_go(window, away, cx);
    assert_eq!(spaces(cx), (ids, 0), "the order and the space shown as they were");
}

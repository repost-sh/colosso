//! The details card's handles: its left edge, its bottom edge and the corner between them size
//! it, kept in settings once they're let go, and a double click puts a side back.

use gpui_kit::{
    AnyWindowHandle, Bounds, CursorStyle, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    Pixels, Point, TestAppContext, VisualTestContext, point, px,
};

use super::drawing::{card_up, shown_chat};
use crate::settings;
use crate::test_support::{center_of, cursor_at, drag, draw, move_held};

const LEFT: &str = "chat-details-resize-left";
const BOTTOM: &str = "chat-details-resize-bottom";
const CORNER: &str = "chat-details-resize-corner";

/// A window showing a settled chat, its card up.
fn with_card(name: &str, cx: &mut TestAppContext) -> AnyWindowHandle {
    let (window, view, _) = shown_chat(name, cx);
    card_up(window, &view, cx);
    draw(window, cx);
    window
}

fn card(window: AnyWindowHandle, cx: &mut TestAppContext) -> Bounds<Pixels> {
    let card = VisualTestContext::from_window(window, cx).debug_bounds("chat-details-card");
    card.expect("the card is up")
}

/// The card's width and height as settings keep them.
fn kept(cx: &mut TestAppContext) -> (Option<f32>, Option<f32>) {
    cx.update(|cx| {
        let details = &settings::get(cx).chat_details;
        (details.width, details.height)
    })
}

/// Drags the middle of `handle` `by` as a person does, and draws what that changed.
fn drag_by(window: AnyWindowHandle, handle: &'static str, by: Point<Pixels>, cx: &mut TestAppContext) {
    let from = center_of(window, handle, cx);
    drag(window, from, from + by, cx);
}

fn double_click(window: AnyWindowHandle, handle: &'static str, cx: &mut TestAppContext) {
    let at = center_of(window, handle, cx);
    let modifiers = Modifiers::default();
    let cx = &mut VisualTestContext::from_window(window, cx);
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            position: at,
            button: MouseButton::Left,
            modifiers,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            button: MouseButton::Left,
            modifiers,
            click_count,
        });
    }
}

fn near(a: Pixels, b: f32) -> bool {
    (a.as_f32() - b).abs() < 0.5
}

#[gpui_kit::test]
fn the_left_edge_widens_the_card_as_it_goes_and_is_kept_once_let_go(cx: &mut TestAppContext) {
    let window = with_card("chat-details-resize-left", cx);
    let at = center_of(window, LEFT, cx);
    assert_eq!(cursor_at(window, at, cx), CursorStyle::ResizeLeftRight);
    let before = card(window, cx);
    assert!(near(before.size.width, 280.), "it comes 280 wide");

    let none = Modifiers::default();
    VisualTestContext::from_window(window, cx).simulate_mouse_down(at, MouseButton::Left, none);
    for step in 1..=6 {
        move_held(window, at - point(px(10. * step as f32), px(0.)), cx);
    }
    draw(window, cx);
    let dragged = card(window, cx);
    assert!(near(dragged.size.width, 340.), "{:?}", dragged.size);
    assert!(near(dragged.right(), before.right().as_f32()), "its right stays put");
    assert_eq!(kept(cx), (None, None), "nothing's kept mid-drag");

    let to = at - point(px(60.), px(0.));
    VisualTestContext::from_window(window, cx).simulate_mouse_up(to, MouseButton::Left, none);
    draw(window, cx);
    assert_eq!(kept(cx), (Some(340.), None));
    assert!(near(card(window, cx).size.width, 340.));
}

#[gpui_kit::test]
fn the_bottom_edge_gives_the_card_a_height_and_the_corner_both(cx: &mut TestAppContext) {
    let window = with_card("chat-details-resize-bottom", cx);
    let bottom = center_of(window, BOTTOM, cx);
    assert_eq!(cursor_at(window, bottom, cx), CursorStyle::ResizeUpDown);
    let corner = center_of(window, CORNER, cx);
    assert_eq!(
        cursor_at(window, corner, cx),
        CursorStyle::ResizeUpRightDownLeft
    );
    let shown = card(window, cx).size.height.as_f32();

    drag_by(window, BOTTOM, point(px(0.), px(40.)), cx);
    let (width, height) = kept(cx);
    assert_eq!(width, None, "the bottom doesn't size the width");
    assert!(height.is_some_and(|height| (height - shown - 40.).abs() < 0.5), "{shown} to {height:?}");
    assert!(near(card(window, cx).size.height, shown + 40.));

    drag_by(window, CORNER, point(px(-30.), px(-20.)), cx);
    let size = card(window, cx).size;
    assert!(near(size.width, 310.) && near(size.height, shown + 20.), "{size:?}");
    assert_eq!(kept(cx), (Some(310.), Some(shown + 20.)));
}

#[gpui_kit::test]
fn the_card_is_kept_within_its_limits(cx: &mut TestAppContext) {
    let window = with_card("chat-details-resize-limits", cx);
    drag_by(window, LEFT, point(px(200.), px(0.)), cx);
    assert!(near(card(window, cx).size.width, 240.), "the narrowest");
    drag_by(window, LEFT, point(px(-2000.), px(0.)), cx);
    assert!(near(card(window, cx).size.width, 640.), "the widest");
    assert_eq!(kept(cx).0, Some(640.));

    drag_by(window, BOTTOM, point(px(0.), px(-2000.)), cx);
    assert!(near(card(window, cx).size.height, 160.), "the shortest");
    drag_by(window, BOTTOM, point(px(0.), px(2000.)), cx);
    let tallest = card(window, cx);
    let pane = cx.update(|cx| window.update(cx, |_, window, _| window.viewport_size()));
    let pane = pane.expect("the window");
    assert!(
        tallest.bottom().as_f32() <= pane.height.as_f32() - 180.,
        "it leaves the composer its room: {tallest:?} in {pane:?}"
    );
    drag_by(window, BOTTOM, point(px(0.), px(500.)), cx);
    assert!(
        near(card(window, cx).size.height, tallest.size.height.as_f32()),
        "it goes no taller"
    );
}

#[gpui_kit::test]
fn a_double_click_puts_a_handles_sides_back(cx: &mut TestAppContext) {
    let window = with_card("chat-details-resize-reset", cx);
    let shown = card(window, cx).size;
    drag_by(window, CORNER, point(px(-60.), px(50.)), cx);
    assert_eq!(kept(cx), (Some(340.), Some(shown.height.as_f32() + 50.)));

    double_click(window, LEFT, cx);
    draw(window, cx);
    assert_eq!(kept(cx).0, None, "the width's back");
    assert!(kept(cx).1.is_some(), "the height stays");
    assert!(near(card(window, cx).size.width, 280.));

    drag_by(window, LEFT, point(px(-60.), px(0.)), cx);
    double_click(window, CORNER, cx);
    draw(window, cx);
    assert_eq!(kept(cx), (None, None));
    let back = card(window, cx).size;
    assert!(
        near(back.width, shown.width.as_f32()) && near(back.height, shown.height.as_f32()),
        "{shown:?} again, not {back:?}"
    );
}

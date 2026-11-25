//! The pointer as a person moves it in a test window: resting on something, a drag from one
//! point to another, and what's taken up, carried somewhere and let go there, drawn at each step
//! as a person's drag is drawn.

use gpui_kit::{
    AnyWindowHandle, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollEvent,
    TestAppContext, TouchPhase, VisualTestContext, point, px,
};

use super::{center_of, draw};

/// Moves the pointer to `at` in `window`, with no button down.
pub fn hover(window: AnyWindowHandle, at: Point<Pixels>, cx: &mut TestAppContext) {
    VisualTestContext::from_window(window, cx).simulate_mouse_move(at, None, Modifiers::default());
}

/// Moves the pointer to `at` in `window`, the left button held.
pub fn move_held(window: AnyWindowHandle, at: Point<Pixels>, cx: &mut TestAppContext) {
    VisualTestContext::from_window(window, cx).simulate_mouse_move(
        at,
        MouseButton::Left,
        Modifiers::default(),
    );
}

/// How far from an edge its handle is grabbed at its middle: half the handle's 6 points, the
/// border inside it, and a point to spare.
pub const GRAB: f32 = 5.;

/// Drags in `window` from `from` to `to` as a person does: the button pressed, the pointer
/// nudged past where a drag starts, moved on to `to`, the button let go, and what that changed
/// drawn.
pub fn drag(
    window: AnyWindowHandle,
    from: Point<Pixels>,
    to: Point<Pixels>,
    cx: &mut TestAppContext,
) {
    let none = Modifiers::default();
    let way = to - from;
    let nudge = from + way * (5. / way.x.as_f32().hypot(way.y.as_f32()).max(1.));
    VisualTestContext::from_window(window, cx).simulate_mouse_down(from, MouseButton::Left, none);
    move_held(window, nudge, cx);
    move_held(window, to, cx);
    VisualTestContext::from_window(window, cx).simulate_mouse_up(to, MouseButton::Left, none);
    draw(window, cx);
}

/// Takes hold of what's at `at` in `window`, as a person starting a drag does: the button
/// pressed there and the pointer nudged past where a drag starts, then drawn, with what's there
/// in hand.
pub fn hold(window: AnyWindowHandle, at: Point<Pixels>, cx: &mut TestAppContext) {
    let none = Modifiers::default();
    VisualTestContext::from_window(window, cx).simulate_mouse_down(at, MouseButton::Left, none);
    move_held(window, at + point(px(3.), px(3.)), cx);
    draw(window, cx);
}

/// Takes up what `from` names by its middle and carries it well off whatever list or grid it's
/// on, over the panes, as a person dragging it elsewhere does: what takes it shows from then.
pub fn take_up(window: AnyWindowHandle, from: &'static str, cx: &mut TestAppContext) {
    hold(window, center_of(window, from, cx), cx);
    move_held(window, point(px(900.), px(450.)), cx);
    draw(window, cx);
}

/// Carries what's in hand to `at` without letting go, drawn three times as the pointer rests
/// there: what it's over has made room for it by then.
pub fn carry(window: AnyWindowHandle, at: Point<Pixels>, cx: &mut TestAppContext) {
    for _ in 0..3 {
        move_held(window, at, cx);
        draw(window, cx);
    }
}

/// Lets go at `at` of what's in hand, and draws what that changed.
pub fn let_go(window: AnyWindowHandle, at: Point<Pixels>, cx: &mut TestAppContext) {
    let none = Modifiers::default();
    VisualTestContext::from_window(window, cx).simulate_mouse_up(at, MouseButton::Left, none);
    draw(window, cx);
}

/// Carries what's in hand over the middle of what `to` names, as it's drawn now, and lets it go.
pub fn let_go_on(window: AnyWindowHandle, to: &'static str, cx: &mut TestAppContext) {
    let target = center_of(window, to, cx);
    move_held(window, target, cx);
    draw(window, cx);
    let_go(window, target, cx);
}

/// Two fingers on the trackpad over the middle of the sidebar, in `phase`, moved `dx` across, and
/// what that changed drawn.
pub fn fingers(window: AnyWindowHandle, phase: TouchPhase, dx: f32, cx: &mut TestAppContext) {
    let mut vcx = VisualTestContext::from_window(window, cx);
    let sidebar = vcx.debug_bounds("sidebar").expect("the sidebar");
    vcx.simulate_event(ScrollEvent {
        position: sidebar.center(),
        delta: ScrollDelta::Pixels(point(px(dx), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: phase,
        ..ScrollEvent::default()
    });
    draw(window, cx);
}

//! A page drawn again after its mode changed: filled again once the device bar was drawn,
//! measured once told its size, a grip held and let go outside the window, and a free size on
//! a denser screen.

use gpui_kit::{
    Bounds, Modifiers, MouseButton, MouseDownEvent, TestAppContext, VisualTestContext, point, px,
    size,
};

use super::super::devices::{self, find};
use super::super::{Change, Choice, Device};
use super::{iphone, page};
use crate::browser::control::Actor;
use crate::browser::view::BrowserView;
use crate::responsive::{Grip, Resizable as _};
use crate::test_support::draw;

fn agent() -> Actor {
    Actor {
        id: "a".into(),
        label: "claude-code:7f3a".into(),
    }
}

fn device(device: &'static Device) -> Change {
    Change {
        device: Some(Choice::Preset(device)),
        ..Change::default()
    }
}

#[gpui_kit::test]
fn filling_again_after_the_mode_was_drawn_tells_the_page_the_whole_pane(cx: &mut TestAppContext) {
    let (window, view) = page(cx);
    let filling = view.read_with(cx, |view, _| view.viewport).unwrap();
    view.update(cx, |view, cx| view.pick_device(Some(iphone()), cx));
    draw(window, cx);
    let told = view.update(cx, |view, cx| {
        view.set_responsive(None, cx);
        view.viewport
    });
    assert_eq!(told, Some(filling), "told the whole pane at once");
    draw(window, cx);
    let drawn = view.read_with(cx, |view, _| view.viewport);
    assert_eq!(drawn, Some(filling), "and not resized again once drawn");

    let agent = agent();
    let phone = device(iphone());
    (view.update(cx, |view, cx| view.agent_viewport(&agent, &phone, cx))).unwrap();
    draw(window, cx);
    let fill = Change {
        fill: true,
        ..Change::default()
    };
    let target = view.update(cx, |view, cx| view.agent_viewport(&agent, &fill, cx));
    let target = target.unwrap().unwrap();
    assert_eq!(
        target, filling,
        "what the agent waits for is the whole pane"
    );
    draw(window, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.viewport,
            Some(target),
            "the page lays out at it once drawn"
        );
        let inner = (f64::from(target.width), f64::from(target.height));
        assert!(view.laid_out_at(target, inner));
    });
}

#[gpui_kit::test]
fn a_page_is_measured_once_told_its_size_and_only_of_that_size(cx: &mut TestAppContext) {
    let (window, view) = page(cx);
    let agent = agent();
    let phone = device(iphone());
    let shown = view.update(cx, |view, cx| view.agent_viewport(&agent, &phone, cx));
    let shown = shown.unwrap().unwrap();
    draw(window, cx);
    view.read_with(cx, |view, _| {
        assert!(view.ready_to_measure(shown), "its own report says it's there");
    });
    view.update(cx, |view, cx| view.set_visible(false, cx));
    let tablet = device(find("ipad-mini").unwrap());
    let target = view.update(cx, |view, cx| view.agent_viewport(&agent, &tablet, cx));
    let target = target.unwrap().unwrap();
    view.read_with(cx, |view, _| {
        assert!(view.ready_to_measure(target));
        assert!(
            !view.ready_to_measure(shown),
            "only of the size it was told"
        );
    });
}

#[gpui_kit::test]
fn a_grip_let_go_outside_the_window_sizes_nothing_more(cx: &mut TestAppContext) {
    let (window, view) = page(cx);
    view.update(cx, BrowserView::toggle_responsive);
    draw(window, cx);
    let (grip, width) = view.read_with(cx, |view, _| {
        let mode = view.responsive.mode.as_ref().unwrap();
        let fit = view.responsive.fit;
        let shown = size(px(mode.width as f32 * fit), px(mode.height as f32 * fit));
        let right = Grip::Right.bounds(Bounds::new(view.origin, shown));
        (right.center(), mode.width)
    });
    let width_now = |cx: &mut TestAppContext| {
        view.read_with(cx, |view, _| view.responsive.mode.as_ref().unwrap().width)
    };
    let press = MouseDownEvent {
        position: grip,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    };
    view.update(cx, |view, cx| view.take_grip(Grip::Right, &press, cx));
    let (none, held) = (Modifiers::default(), Some(MouseButton::Left));
    let mut pointer = VisualTestContext::from_window(window, cx);
    pointer.simulate_mouse_move(grip - point(px(50.), px(0.)), held, none);
    let dragged = width_now(cx);
    assert_eq!(
        dragged,
        width - 100,
        "the page stays centred, its edge under the pointer"
    );
    let mut pointer = VisualTestContext::from_window(window, cx);
    pointer.simulate_mouse_move(grip - point(px(150.), px(0.)), None, none);
    assert_eq!(width_now(cx), dragged, "the grip is let go");
    assert!(view.read_with(cx, |view, _| view.responsive.drag.is_none()));
}

#[gpui_kit::test]
fn a_grip_held_sizes_the_page_as_the_pane_is_drawn(cx: &mut TestAppContext) {
    let (window, view) = page(cx);
    view.update(cx, BrowserView::toggle_responsive);
    draw(window, cx);
    let (grip, before) = view.read_with(cx, |view, _| {
        let mode = view.responsive.mode.as_ref().unwrap();
        let fit = view.responsive.fit;
        let shown = size(px(mode.width as f32 * fit), px(mode.height as f32 * fit));
        let right = Grip::Right.bounds(Bounds::new(view.origin, shown));
        (right.center(), view.viewport.unwrap())
    });
    let press = MouseDownEvent {
        position: grip,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    };
    view.update(cx, |view, cx| view.take_grip(Grip::Right, &press, cx));
    let (none, held) = (Modifiers::default(), Some(MouseButton::Left));
    let mut pointer = VisualTestContext::from_window(window, cx);
    pointer.simulate_mouse_move(grip - point(px(50.), px(0.)), held, none);
    let dragged = view.read_with(cx, |view, _| view.responsive.mode.as_ref().unwrap().width);
    assert_ne!(dragged, before.width, "the grip sized the mode");
    draw(window, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.viewport.map(|viewport| viewport.width), Some(dragged));
    });
    let mut pointer = VisualTestContext::from_window(window, cx);
    pointer.simulate_mouse_move(grip - point(px(50.), px(0.)), None, none);
    draw(window, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.viewport.map(|viewport| viewport.width),
            Some(dragged),
            "let go, it stays at the size it ended at"
        );
        assert!(view.responsive.drag.is_none());
    });
}

#[gpui_kit::test]
fn a_free_size_stays_within_what_a_denser_screen_shows(cx: &mut TestAppContext) {
    let (window, view) = page(cx);
    cx.simulate_window_scale_factor_change(window, 1.);
    draw(window, cx);
    let agent = agent();
    let wide = Change {
        width: Some(10_000),
        height: Some(700),
        ..Change::default()
    };
    (view.update(cx, |view, cx| view.agent_viewport(&agent, &wide, cx))).unwrap();
    draw(window, cx);
    let width = view.read_with(cx, |view, _| view.viewport.map(|viewport| viewport.width));
    assert_eq!(width, Some(10_000));
    cx.simulate_window_scale_factor_change(window, 2.);
    draw(window, cx);
    view.read_with(cx, |view, _| {
        let viewport = view.viewport.unwrap();
        assert!(viewport.width * 2 <= 16_384, "{viewport:?}");
        let mode = view.responsive.mode.as_ref().unwrap();
        assert_eq!(mode.width, devices::max_side(2.), "the widest a 2× page is");
    });
    let taller = Change {
        height: Some(900),
        ..Change::default()
    };
    let taller = view.update(cx, |view, cx| view.agent_viewport(&agent, &taller, cx));
    assert!(taller.is_ok(), "later changes still go: {taller:?}");
    view.update(cx, |view, cx| view.person_sizes(4000, 900, cx));
    draw(window, cx);
    let width = view.read_with(cx, |view, _| view.viewport.map(|viewport| viewport.width));
    assert_eq!(width, Some(4000), "and so do the person's");
}

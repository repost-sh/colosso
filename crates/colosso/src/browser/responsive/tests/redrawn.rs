//! A page drawn again after its mode changed: filled again once the device bar was drawn, out
//! of sight, an old frame at its own scale until a new one comes, a grip let go outside the
//! window, and a free size on a denser screen.

use colosso_browser_ipc::{PixelSize, Viewport};
use gpui_kit::{
    Bounds, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, PlatformInput, TestAppContext,
    VisualTestContext, point, px, size,
};

use super::super::devices::{self, find};
use super::super::{Change, Choice, Device, frame_scale};
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
    // Drawn with the mode on, the page's room is under the device bar.
    draw(window, cx);
    // The person's ✕, and the size the page is told with it, before the window draws again.
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
fn a_page_out_of_sight_is_measured_without_a_frame(cx: &mut TestAppContext) {
    let (window, view) = page(cx);
    let agent = agent();
    let phone = device(iphone());
    let shown = view.update(cx, |view, cx| view.agent_viewport(&agent, &phone, cx));
    let shown = shown.unwrap().unwrap();
    draw(window, cx);
    view.update(cx, |view, cx| view.set_visible(true, cx));
    view.read_with(cx, |view, _| {
        assert!(
            !view.ready_to_measure(shown),
            "on screen, it waits for a frame of its new size"
        );
    });
    // Its tab goes out of sight: the host stops painting it.
    view.update(cx, |view, cx| view.set_visible(false, cx));
    let tablet = device(find("ipad-mini").unwrap());
    let target = view.update(cx, |view, cx| view.agent_viewport(&agent, &tablet, cx));
    let target = target.unwrap().unwrap();
    view.read_with(cx, |view, _| {
        assert!(
            view.ready_to_measure(target),
            "no frame comes: its own report is all there is"
        );
        assert!(
            !view.ready_to_measure(shown),
            "only of the size it was told"
        );
    });
}

#[test]
fn an_old_frame_shows_at_its_own_scale_until_one_of_the_new_size_comes() {
    let is = |found: f32, wanted: f32| (found - wanted).abs() < f32::EPSILON;
    let at = |width, height| PixelSize { width, height };
    let phone = Viewport {
        width: 393,
        height: 852,
        scale: 3.,
    };
    assert!(is(frame_scale(at(1179, 2556), Some(phone), None), 3.));
    // A free size at the window's 2×, the phone's frame still on screen.
    let free = Viewport { scale: 2., ..phone };
    let old = frame_scale(at(1179, 2556), Some(free), Some(3.));
    assert!(is(old, 3.), "the old frame's own: {old}");
    let new = frame_scale(at(786, 1704), Some(free), Some(3.));
    assert!(is(new, 2.), "the new one's: {new}");
    // Mid-drag, frames of a size the page has since left share the scale of the one before.
    let wider = Viewport { width: 500, ..free };
    let between = frame_scale(at(800, 1704), Some(wider), Some(2.));
    assert!(is(between, 2.), "{between}");
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
    // Taken straight from the view: a test's page is under the notice that its engine
    // didn't start, which keeps the pointer from the grip.
    let press = MouseDownEvent {
        position: grip,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    };
    view.update(cx, |view, cx| view.take_grip(Grip::Right, &press, cx));
    // The window's moves carry it.
    let (none, held) = (Modifiers::default(), Some(MouseButton::Left));
    let mut pointer = VisualTestContext::from_window(window, cx);
    pointer.simulate_mouse_move(grip - point(px(50.), px(0.)), held, none);
    let dragged = width_now(cx);
    assert_eq!(
        dragged,
        width - 100,
        "the page stays centred, its edge under the pointer"
    );
    // Let go outside the window, the button comes up unseen: the next move has none held.
    let mut pointer = VisualTestContext::from_window(window, cx);
    pointer.simulate_mouse_move(grip - point(px(150.), px(0.)), None, none);
    assert_eq!(width_now(cx), dragged, "the grip is let go");
    assert!(view.read_with(cx, |view, _| view.responsive.drag.is_none()));
    // Over the page its native view hands the pointer on, the same way.
    view.update(cx, |view, cx| view.take_grip(Grip::Right, &press, cx));
    let moved = |to, pressed_button| {
        PlatformInput::MouseMove(MouseMoveEvent {
            position: to,
            pressed_button,
            modifiers: Modifiers::default(),
        })
    };
    let released = moved(grip - point(px(150.), px(0.)), None);
    view.update(cx, |view, cx| assert!(view.grip_pointer(&released, cx)));
    assert_eq!(width_now(cx), dragged, "let go there too");
    assert!(view.read_with(cx, |view, _| view.responsive.drag.is_none()));
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
    let pixels = view.read_with(cx, |view, _| {
        view.viewport.map(|viewport| viewport.pixels())
    });
    assert_eq!(pixels.map(|pixels| pixels.width), Some(10_000));
    // The window moves to a 2× screen.
    cx.simulate_window_scale_factor_change(window, 2.);
    draw(window, cx);
    view.read_with(cx, |view, _| {
        let viewport = view.viewport.unwrap();
        assert!(viewport.pixels().width <= 16_384, "{viewport:?}");
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
    let width = view.read_with(cx, |view, _| view.viewport.map(|viewport| viewport.width));
    assert_eq!(width, Some(4000), "and so do the person's");
}

//! The person's hand on a device's tab against a stand-in host (QP1-QP6): what the window
//! hears, in a real test window with the tab drawn, becomes the `Input` the host is sent.

use std::sync::mpsc::Receiver;
use std::time::Duration;

use gpui_kit::{
    AppContext as _, AnyWindowHandle, Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PlatformInput, Pixels, Point, ScrollDelta, ScrollEvent, TestAppContext, TouchPhase, point, px,
};
use koyo_device_wire::{Button, Input, ToHost};

use super::super::link::Link;
use super::super::view::{Chosen, DeviceView};
use super::screen;
use crate::test_support::{draw, repo, select, shell_with};

struct Tab {
    window: AnyWindowHandle,
    view: Entity<DeviceView>,
    told: Receiver<ToHost>,
}

/// A device's tab in the shell, shown, its stand-in host standing ready and the picture drawn.
fn tab(name: &str, cx: &mut TestAppContext) -> Tab {
    let root = repo(name, &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let chosen = Chosen {
        udid: "UDID-HAND".into(),
        name: "iPhone 17 Pro".into(),
        ipad: false,
    };
    shell.update(cx, |shell, cx| shell.device_pane(Some(chosen), true, cx));
    let view = cx.read(|cx| shell.read(cx).shown_device_view(cx)).expect("the device's tab");
    let link: Entity<Link> = cx.read(|cx| view.read(cx).link().cloned()).expect("its link");
    let told = link.update(cx, |link, cx| link.stand_in(screen(), cx));
    draw(window, cx);
    Tab { window, view, told }
}

impl Tab {
    /// Where the window draws the screen's point (`x`, `y`), in the device's points.
    fn at(&self, x: f32, y: f32, cx: &TestAppContext) -> Point<Pixels> {
        let (origin, fit) = cx.read(|cx| self.view.read(cx).placed).expect("the picture placed");
        point(origin.x + px(x * fit), origin.y + px(y * fit))
    }

    fn event(&self, event: PlatformInput, cx: &mut TestAppContext) {
        (cx.update_window(self.window, |_, window, cx| {
            window.dispatch_event(event, cx);
        }))
        .expect("the window");
        cx.run_until_parked();
    }

    fn press(&self, at: Point<Pixels>, cx: &mut TestAppContext) {
        self.event(
            PlatformInput::MouseDown(MouseDownEvent {
                position: at,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    }

    fn drag_to(&self, at: Point<Pixels>, cx: &mut TestAppContext) {
        self.event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: at,
                modifiers: Modifiers::none(),
                pressed_button: Some(MouseButton::Left),
            }),
            cx,
        );
    }

    fn release(&self, at: Point<Pixels>, cx: &mut TestAppContext) {
        self.event(
            PlatformInput::MouseUp(MouseUpEvent {
                position: at,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count: 1,
            }),
            cx,
        );
    }

    /// What the host was told since, steps that come after a gap included.
    fn told(&self, cx: &mut TestAppContext) -> Vec<ToHost> {
        cx.executor().advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
        self.told.try_iter().collect()
    }

    fn inputs(&self, cx: &mut TestAppContext) -> Vec<Input> {
        (self.told(cx).into_iter())
            .filter_map(|told| match told {
                ToHost::Input { input } => Some(input),
                _ => None,
            })
            .collect()
    }
}

/// Each `Touches` of `inputs` as its first finger's place and whether it touches.
fn touches(inputs: &[Input]) -> Vec<(f64, f64, bool)> {
    (inputs.iter())
        .filter_map(|input| match input {
            Input::Touches { fingers, .. } => fingers.first().map(|f| (f.at.x, f.at.y, f.down)),
            _ => None,
        })
        .collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.5
}

/// QP1: a click is a tap where it was pressed; the tab drawing the picture told the host so.
#[gpui_kit::test]
fn a_click_on_the_picture_taps_the_device_there(cx: &mut TestAppContext) {
    let tab = tab("device-hand-click", cx);
    let told = tab.told(cx);
    assert!(told.contains(&ToHost::Visible { visible: true }), "{told:?}");
    let at = tab.at(201., 437., cx);
    tab.press(at, cx);
    tab.release(at, cx);
    let touches = touches(&tab.inputs(cx));
    assert!(touches.len() >= 2, "{touches:?}");
    let (first, last) = (touches[0], touches[touches.len() - 1]);
    assert!(first.2 && near(first.0, 201.) && near(first.1, 437.), "{touches:?}");
    assert!(!last.2 && near(last.0, 201.) && near(last.1, 437.), "{touches:?}");
}

/// QP3: a drag moves one finger along the way it went and lifts it where it ended.
#[gpui_kit::test]
fn a_drag_moves_one_finger_and_lifts_it_where_it_ended(cx: &mut TestAppContext) {
    let tab = tab("device-hand-drag", cx);
    let _ = tab.told(cx);
    tab.press(tab.at(200., 700., cx), cx);
    for y in [600., 500., 400., 300.] {
        tab.drag_to(tab.at(200., y, cx), cx);
    }
    tab.release(tab.at(200., 300., cx), cx);
    let touches = touches(&tab.inputs(cx));
    assert!(touches.len() >= 5, "{touches:?}");
    let ys: Vec<f64> = touches.iter().map(|t| t.1).collect();
    assert!(ys.windows(2).all(|pair| pair[1] <= pair[0] + 0.5), "{ys:?}");
    let last = touches[touches.len() - 1];
    assert!(!last.2 && near(last.1, 300.), "{touches:?}");
}

/// QP4: a trackpad's scroll over the picture is a finger dragged on the device.
#[gpui_kit::test]
fn a_scroll_over_the_picture_drags_a_finger(cx: &mut TestAppContext) {
    let tab = tab("device-hand-scroll", cx);
    let _ = tab.told(cx);
    let at = tab.at(201., 437., cx);
    let scroll = |phase, by: f32| {
        PlatformInput::Scroll(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(0.), px(by))),
            modifiers: Modifiers::none(),
            touch_phase: phase,
            source: gpui_kit::ScrollSource::Pointer,
        })
    };
    tab.event(scroll(TouchPhase::Started, 0.), cx);
    for _ in 0..4 {
        tab.event(scroll(TouchPhase::Moved, -30.), cx);
    }
    tab.event(scroll(TouchPhase::Ended, 0.), cx);
    let touches = touches(&tab.inputs(cx));
    assert!(touches.len() >= 3, "{touches:?}");
    assert!(touches.first().is_some_and(|t| t.2), "{touches:?}");
    assert!(touches.last().is_some_and(|t| !t.2), "{touches:?}");
    let moved = touches.last().map_or(0., |t| t.1) - touches[0].1;
    assert!(moved.abs() > 20., "the finger moved {moved} points: {touches:?}");
}

/// QP5: text typed with the tab's picture focused goes to the device as text.
#[gpui_kit::test]
fn text_typed_in_the_tab_is_typed_on_the_device(cx: &mut TestAppContext) {
    let tab = tab("device-hand-type", cx);
    let at = tab.at(201., 437., cx);
    tab.press(at, cx);
    tab.release(at, cx);
    draw(tab.window, cx);
    let _ = tab.told(cx);
    cx.simulate_input(tab.window, "hi");
    let typed: String = (tab.inputs(cx).into_iter())
        .filter_map(|input| match input {
            Input::Text { text } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(typed, "hi");
}

/// QP6: ⇧⌘H is the device's Home, Escape its own key, and ⌘T still Koyo's.
#[gpui_kit::test]
fn shortcuts_reach_the_device_but_koyo_keeps_its_own(cx: &mut TestAppContext) {
    let tab = tab("device-hand-keys", cx);
    let at = tab.at(201., 437., cx);
    tab.press(at, cx);
    tab.release(at, cx);
    draw(tab.window, cx);
    let _ = tab.told(cx);
    cx.simulate_keystrokes(tab.window, "cmd-shift-h");
    let inputs = tab.inputs(cx);
    assert!(
        inputs.contains(&Input::Button { button: Button::Home, down: true })
            && inputs.contains(&Input::Button { button: Button::Home, down: false }),
        "{inputs:?}"
    );
    cx.simulate_keystrokes(tab.window, "escape");
    let inputs = tab.inputs(cx);
    assert_eq!(
        inputs,
        [Input::Key { usage: 41, down: true }, Input::Key { usage: 41, down: false }]
    );
    cx.simulate_keystrokes(tab.window, "cmd-t");
    assert_eq!(tab.inputs(cx), [], "⌘T opened Koyo's launcher, not the device's");
}

/// QP2: a press held is a finger held: nothing lifts it until the button comes up.
#[gpui_kit::test]
fn a_press_held_keeps_the_finger_down_until_let_go(cx: &mut TestAppContext) {
    let tab = tab("device-hand-hold", cx);
    let _ = tab.told(cx);
    let at = tab.at(100., 200., cx);
    tab.press(at, cx);
    cx.executor().advance_clock(Duration::from_secs(2));
    let held = touches(&tab.inputs(cx));
    assert!(!held.is_empty() && held.iter().all(|t| t.2), "lifted while held: {held:?}");
    tab.release(at, cx);
    let let_go = touches(&tab.inputs(cx));
    assert!(let_go.last().is_some_and(|t| !t.2 && near(t.0, 100.) && near(t.1, 200.)), "{let_go:?}");
}

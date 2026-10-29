//! A held arrow in a window that tracks releases, driven by frames rather than by repeats:
//! the frames are delivered by hand, and the clock moved between them.

use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Capslock, Context, Entity, FocusHandle, IntoElement,
    KeyBinding, KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent, PlatformInput, Render,
    Styled as _, TestAppContext, Window, actions, div, px, size,
};

use super::*;
use crate::test_support::in_window;

actions!(key_repeat_test, [Down, Back]);

/// A list that counts its steps down, and a field that counts its deletes.
struct List {
    steps: usize,
    deletes: usize,
    focus: FocusHandle,
}

impl Render for List {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        track(div(), window, cx)
            .track_focus(&self.focus)
            .key_context("RepeatList")
            .on_action(cx.listener(|this, _: &Down, _, cx| {
                this.steps += 1;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Back, _, _| this.deletes += 1))
            .size_full()
    }
}

/// How long a frame is at 120 Hz, near enough.
const FRAME: Duration = Duration::from_millis(8);

fn list(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<List>) {
    list_with(KeyRepeat::default(), cx)
}

/// The list in a window of its own, a held arrow repeating as `repeat` says.
fn list_with(repeat: KeyRepeat, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<List>) {
    cx.update(|cx| {
        settings::init(cx);
        settings::update(cx, |settings| settings.key_repeat = repeat);
        init(cx);
        let list = Some("RepeatList");
        cx.bind_keys([
            KeyBinding::new("down", Down, list),
            KeyBinding::new("backspace", Back, list),
        ]);
    });
    let (window, list) = in_window(size(px(200.), px(200.)), cx, |window, cx| {
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            List {
                steps: 0,
                deletes: 0,
                focus,
            }
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.activate_window();
        window.draw(cx).clear(cx);
    })
    .unwrap();
    cx.run_until_parked();
    (window, list)
}

fn press(key: &'static str, window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.dispatch_keystroke(window, Keystroke::parse(key).unwrap());
    cx.run_until_parked();
}

fn release(key: &'static str, window: AnyWindowHandle, cx: &mut TestAppContext) {
    let keystroke = Keystroke::parse(key).unwrap();
    cx.update_window(window, |_, window, cx| {
        window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
    })
    .unwrap();
}

/// `count` frames, `every` apart, each drawn and then what waits for it run.
fn frames(count: usize, every: Duration, window: AnyWindowHandle, cx: &mut TestAppContext) {
    for _ in 0..count {
        cx.executor().advance_clock(every);
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

fn steps(list: &Entity<List>, cx: &mut TestAppContext) -> usize {
    list.read_with(cx, |list, _| list.steps)
}

#[gpui_kit::test]
fn a_tap_acts_once(cx: &mut TestAppContext) {
    let (window, list) = list(cx);
    press("down", window, cx);
    release("down", window, cx);
    frames(60, FRAME, window, cx);
    assert_eq!(steps(&list, cx), 1);
}

#[gpui_kit::test]
fn a_held_arrow_repeats_once_the_delay_has_passed(cx: &mut TestAppContext) {
    let (window, list) = list(cx);
    press("down", window, cx);
    // Nothing more until the delay's passed.
    frames(18, FRAME, window, cx);
    assert_eq!(steps(&list, cx), 1);
    frames(30, FRAME, window, cx);
    let held = steps(&list, cx);
    assert!(held > 1, "{held} steps");
    // macOS's own repeats of it go no further.
    press("down", window, cx);
    press("down", window, cx);
    assert_eq!(steps(&list, cx), held);
}

/// How many rows a second an arrow held past its delay moves, `hz` frames a second.
fn rows_a_second(hz: u32, cx: &mut TestAppContext) -> f32 {
    let (window, list) = list(cx);
    press("down", window, cx);
    frames(1, Duration::from_millis(150), window, cx);
    let before = steps(&list, cx);
    frames(2 * hz as usize, Duration::from_secs(1) / hz, window, cx);
    (steps(&list, cx) - before) as f32 / 2.
}

#[gpui_kit::test]
fn at_60_hz_it_moves_its_rows_a_second(cx: &mut TestAppContext) {
    let rate = rows_a_second(60, cx);
    assert!((rate - 96.).abs() <= 0.5, "{rate} rows a second");
}

#[gpui_kit::test]
fn at_120_hz_it_moves_as_many(cx: &mut TestAppContext) {
    let rate = rows_a_second(120, cx);
    assert!((rate - 96.).abs() <= 0.5, "{rate} rows a second");
}

#[gpui_kit::test]
fn letting_go_stops_it(cx: &mut TestAppContext) {
    let (window, list) = list(cx);
    press("down", window, cx);
    frames(1, Duration::from_millis(200), window, cx);
    frames(5, FRAME, window, cx);
    let held = steps(&list, cx);
    assert!(held > 1, "{held} steps");
    release("down", window, cx);
    frames(30, FRAME, window, cx);
    assert_eq!(steps(&list, cx), held);
    // And it's a fresh press next time.
    press("down", window, cx);
    assert_eq!(steps(&list, cx), held + 1);
}

#[gpui_kit::test]
fn turned_off_the_arrows_repeat_as_macos_repeats_them(cx: &mut TestAppContext) {
    let off = KeyRepeat {
        enabled: false,
        ..KeyRepeat::default()
    };
    let (window, list) = list_with(off, cx);
    press("down", window, cx);
    frames(60, FRAME, window, cx);
    assert_eq!(steps(&list, cx), 1);
    // macOS's repeats, with no release between, each act.
    press("down", window, cx);
    press("down", window, cx);
    assert_eq!(steps(&list, cx), 3);
}

#[gpui_kit::test]
fn a_changed_modifier_stops_it_until_it_comes_up(cx: &mut TestAppContext) {
    let (window, list) = list(cx);
    press("down", window, cx);
    frames(1, Duration::from_millis(200), window, cx);
    let held = steps(&list, cx);
    assert!(held > 1, "{held} steps");
    let shift = ModifiersChangedEvent {
        modifiers: Modifiers::shift(),
        capslock: Capslock::default(),
    };
    cx.update_window(window, |_, window, cx| {
        window.dispatch_event(PlatformInput::ModifiersChanged(shift), cx);
    })
    .unwrap();
    frames(30, FRAME, window, cx);
    press("down", window, cx);
    assert_eq!(steps(&list, cx), held);
    release("down", window, cx);
    press("down", window, cx);
    assert_eq!(steps(&list, cx), held + 1);
}

#[gpui_kit::test]
fn a_press_after_no_frames_for_a_while_is_a_press_anew(cx: &mut TestAppContext) {
    let (window, list) = list(cx);
    press("down", window, cx);
    // Its release went elsewhere, and the window drew nothing meanwhile.
    cx.executor().advance_clock(Duration::from_millis(300));
    press("down", window, cx);
    assert_eq!(steps(&list, cx), 2);
}

#[gpui_kit::test]
fn other_keys_repeat_as_macos_repeats_them(cx: &mut TestAppContext) {
    let (window, list) = list(cx);
    // Backspace held: macOS's repeats, with no release between, each act.
    for _ in 0..3 {
        press("backspace", window, cx);
    }
    frames(60, FRAME, window, cx);
    assert_eq!(list.read_with(cx, |list, _| list.deletes), 3);
    // Another key while an arrow's held stops the arrow.
    press("down", window, cx);
    press("backspace", window, cx);
    frames(60, FRAME, window, cx);
    assert_eq!(steps(&list, cx), 1);
}

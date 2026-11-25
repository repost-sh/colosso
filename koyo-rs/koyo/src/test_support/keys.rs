//! Keys pressed and let up, and actions dispatched, in a test's window as a person's would be,
//! and the display's frames that follow them, each drawn.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    Action, AnyWindowHandle, AppContext as _, KeyContext, KeyUpEvent, Keystroke, PlatformInput,
    TestAppContext,
};

use super::draw;

/// Dispatches `action` in `window` where the keyboard is, and draws once it's done.
pub fn act(window: AnyWindowHandle, action: impl Action, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(action), cx);
    }))
    .unwrap();
    cx.run_until_parked();
    draw(window, cx);
}

/// Presses `keys` in `window`, drawn first so the keyboard is where the last change left it.
pub fn press(window: AnyWindowHandle, keys: &str, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))).unwrap();
    cx.simulate_keystrokes(window, keys);
}

/// Presses `key` in `window` and lets it up, as a person does: an arrow pressed again before
/// it came up is taken for macOS repeating it (`key_repeat`). Drawn first, so the keyboard is
/// where the last change left it. The action bound to it that ran, if one did, and the key
/// contexts it was pressed in.
pub fn pressed(
    window: AnyWindowHandle,
    key: &'static str,
    cx: &mut TestAppContext,
) -> (Option<&'static str>, Vec<KeyContext>) {
    let heard = Rc::new(RefCell::new(None));
    let hearing = heard.clone();
    let _observing = cx.update(|cx| {
        cx.observe_keystrokes(move |event, _, _| {
            let action = event.action.as_ref().map(|action| action.name());
            *hearing.borrow_mut() = Some((action, event.context_stack.clone()));
        })
    });
    press(window, key, cx);
    let keystroke = Keystroke::parse(key).unwrap();
    (cx.update_window(window, |_, window, cx| {
        window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
    }))
    .unwrap();
    draw(window, cx);
    let heard = heard.borrow_mut().take();
    heard.unwrap_or_else(|| {
        let stack = cx.update_window(window, |_, window, _| window.context_stack());
        (None, stack.unwrap())
    })
}

/// `count` frames of the display's, 16 ms apart, each drawn: a space's slide moving on.
pub fn frames(window: AnyWindowHandle, count: usize, cx: &mut TestAppContext) {
    for _ in 0..count {
        cx.executor().advance_clock(Duration::from_millis(16));
        (cx.update_window(window, |_, window, cx| window.simulate_next_frame(cx))).unwrap();
        draw(window, cx);
    }
}

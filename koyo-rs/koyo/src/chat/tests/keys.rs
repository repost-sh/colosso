//! Where the keyboard is in a chat's tab (`shell/keys.rs`): in its prompt only once the person
//! puts it there, and out of it again when they press anything but the prompt's box.

use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

use super::super::view::ChatView;
use super::drawing::shown_chat;
use crate::test_support::{PROMPT, keyboard_in, press_prompt};

fn typed(view: &Entity<ChatView>, cx: &TestAppContext) -> String {
    view.read_with(cx, |view, cx| {
        let composer = view.composer.read(cx);
        composer.text.read(cx).value().to_string()
    })
}

/// A chat brought back with the window, with what was said in it, the only tab there is.
#[gpui_kit::test]
fn a_chat_brought_back_has_the_keyboard_in_its_prompt_only_once_it_is_pressed(
    cx: &mut TestAppContext,
) {
    let (window, view, _) = shown_chat("chat-keys-back", cx);
    assert!(
        !keyboard_in(window, PROMPT, cx),
        "shown, it doesn't take the keyboard"
    );
    cx.simulate_input(window, "nowhere");
    assert_eq!(typed(&view, cx), "");

    // A press in the prompt: what's typed goes in it.
    press_prompt(window, cx);
    assert!(keyboard_in(window, PROMPT, cx), "pressed in its text");
    cx.simulate_input(window, "hello");
    assert_eq!(typed(&view, cx), "hello");
    // Pressed again where the keyboard is already, it stays.
    press_prompt(window, cx);
    assert!(keyboard_in(window, PROMPT, cx), "pressed in its text again");
    cx.simulate_input(window, " there");
    assert_eq!(typed(&view, cx), "hello there");

    // A press on what was said takes the keyboard out, and it stays out.
    let press = |selector: &'static str, inset: f32, cx: &mut TestAppContext| {
        let mut vcx = VisualTestContext::from_window(window, cx);
        let bounds = vcx.debug_bounds(selector).expect(selector);
        let at = if inset > 0. {
            bounds.origin + point(px(inset), px(inset))
        } else {
            bounds.center()
        };
        vcx.simulate_click(at, Modifiers::default());
    };
    press("chat-row-0", 0., cx);
    assert!(!keyboard_in(window, PROMPT, cx), "pressed what was said");
    cx.simulate_input(window, " lost");
    assert_eq!(typed(&view, cx), "hello there");

    // The box's own edge is the prompt's: a press there puts the keyboard in it.
    press("chat-composer", 5., cx);
    assert!(keyboard_in(window, PROMPT, cx), "pressed the box's edge");
}

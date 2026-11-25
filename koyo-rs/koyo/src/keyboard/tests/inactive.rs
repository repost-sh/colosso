//! The shell in a test window, with a fake `AppKit`: GPUI's focus leaving a page in a frame drawn
//! while the window wasn't key, which GPUI tells the page nothing of.

use std::rc::Rc;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext};

use koyo_seam::keyboard::hooks;
use koyo_seam::keyboard::reconcile::{AppKit as _, Responder};
use koyo_seam::keyboard::state::{Holder, Part};
use koyo_seam::keyboard::testing::{Fake, installed};

use crate::keyboard::{self, key_of};
use crate::shell::Shell;
use crate::test_support::{draw, open_page_view, shell_on};

fn activate(window: AnyWindowHandle, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, _| window.activate_window())).unwrap();
    cx.run_until_parked();
}

/// GPUI's focus moves to the shell's root, as a press on the glass moves it.
fn focus_root(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, cx| {
        let root = shell.read(cx).focus.clone();
        window.focus(&root, cx);
    }))
    .unwrap();
}

/// A shell in a key window whose page, shown and drawn, has the keyboard.
fn on_page(name: &str, cx: &mut TestAppContext) -> (Rc<Fake>, AnyWindowHandle, Entity<Shell>) {
    let (fake, _turns) = installed();
    let (window, shell, _) = shell_on(name, cx);
    activate(window, cx);
    let page = open_page_view(window, &shell, "https://example.com/", cx);
    let page_id = page.read_with(cx, |page, _| page.page_id());
    let key = key_of(window);
    fake.page_in(page_id, Some(key));
    hooks::page_arrived(page_id);
    assert_eq!(keyboard::holder(window), Some(Holder::Page(page_id)));
    assert_eq!(fake.first_responder(key), Responder::Page(page_id));
    (fake, window, shell)
}

/// GPUI's view has the keyboard, and the next key goes to it.
fn gpui_has_the_keyboard(fake: &Fake, window: AnyWindowHandle) {
    let key = key_of(window);
    assert_eq!(keyboard::holder(window), Some(Holder::Gpui(Part::Other)));
    hooks::at_key(key);
    assert_eq!(fake.first_responder(key), Responder::Gpui, "the key goes to GPUI's view");
}

/// Back from the launcher or another app with a press on the composer: GPUI moves its focus as
/// it hears the press, before it hears the window is key again, and tells the page of none of
/// it. The keyboard is GPUI's all the same, and the next key goes to GPUI's view.
#[gpui_kit::test]
fn a_press_that_brings_the_window_back_off_a_page_gives_gpui_the_keyboard(
    cx: &mut TestAppContext,
) {
    let (fake, window, shell) = on_page("keyboard-inactive-press", cx);

    VisualTestContext::from_window(window, cx).deactivate_window();
    draw(window, cx);
    focus_root(window, &shell, cx);
    draw(window, cx);
    activate(window, cx);
    draw(window, cx);

    gpui_has_the_keyboard(&fake, window);
}

/// Back with the window heard key first and the press on the glass after it, both before a
/// frame is drawn with the window key: GPUI tells the page nothing of the move, as the frame
/// before was drawn while it wasn't. The keyboard is GPUI's all the same.
#[gpui_kit::test]
fn a_press_heard_after_the_window_is_key_again_off_a_page_gives_gpui_the_keyboard(
    cx: &mut TestAppContext,
) {
    let (fake, window, shell) = on_page("keyboard-key-then-press", cx);

    VisualTestContext::from_window(window, cx).deactivate_window();
    draw(window, cx);
    (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |_, cx| {
            cx.observe_window_activation(window, |shell, window, cx| {
                if window.is_window_active() {
                    window.focus(&shell.focus, cx);
                }
            })
            .detach();
        });
    }))
    .unwrap();
    activate(window, cx);
    draw(window, cx);

    gpui_has_the_keyboard(&fake, window);
}

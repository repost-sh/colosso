//! The shell in a test window, with a fake `AppKit`: GPUI's focus leaving a page in a frame drawn
//! while the window wasn't key, which GPUI tells the page nothing of.

use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext, VisualTestContext};

use koyo_seam::keyboard::testing::installed;
use koyo_seam::keyboard::reconcile::{AppKit as _, Responder};
use koyo_seam::keyboard::state::{Holder, Part};
use koyo_seam::keyboard::{held, hooks};
use crate::test_support::{draw, open_page, repo, select, shell_with};

fn activate(window: AnyWindowHandle, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, _| window.activate_window())).unwrap();
    cx.run_until_parked();
}

/// Back from the launcher or another app with a press on the composer: GPUI moves its focus as
/// it hears the press, before it hears the window is key again, and tells the page of none of
/// it. The keyboard is GPUI's all the same, and the next key goes to GPUI's view.
#[gpui_kit::test]
fn a_press_that_brings_the_window_back_off_a_page_gives_gpui_the_keyboard(
    cx: &mut TestAppContext,
) {
    let (fake, _turns) = installed();
    let root = repo("keyboard-inactive-press", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    activate(window, cx);
    let pane = open_page(window, &shell, Some("https://example.com/"), cx);
    for _ in 0..3 {
        draw(window, cx);
    }
    let page_id = shell.read_with(cx, |shell, cx| {
        shell.workspace.pane(pane).unwrap().view().unwrap().read(cx).page_id()
    });
    let key = window.window_id().as_u64();
    fake.page_in(page_id, Some(key));
    hooks::page_arrived(page_id);
    assert_eq!(held(key).map(|held| held.0), Some(Holder::Page(page_id)));
    assert_eq!(fake.first_responder(key), Responder::Page(page_id));

    // The launcher's window, or another app, has the keys.
    VisualTestContext::from_window(window, cx).deactivate_window();
    draw(window, cx);
    // The press on the composer moves GPUI's focus while the window isn't key yet.
    (cx.update_window(window, |_, window, cx| {
        let root = shell.read(cx).focus.clone();
        window.focus(&root, cx);
    }))
    .unwrap();
    draw(window, cx);
    activate(window, cx);
    draw(window, cx);

    assert_eq!(held(key).map(|held| held.0), Some(Holder::Gpui(Part::Other)));
    hooks::at_key(key);
    assert_eq!(fake.first_responder(key), Responder::Gpui, "the key goes to GPUI's view");
}

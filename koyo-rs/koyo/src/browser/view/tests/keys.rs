//! ⌘P as `AppKit` hands it to the window, through GPUI's keymap: it prints the page that has
//! the keyboard, as Chrome's File > Print does, and opens Go to File anywhere else.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use koyo_browser_ipc::{AppRequest, Print};

use crate::browser::{BrowserView, FakeHost};
use crate::shell::Shell;
use crate::test_support::{keyboard_in, open_page_view, shell_on};

fn printed(host: &FakeHost, cx: &mut TestAppContext) -> Vec<AppRequest> {
    (host.asked(cx).into_iter())
        .filter(|asked| matches!(asked, AppRequest::Print { .. }))
        .collect()
}

fn windows(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.windows().len())
}

fn keys_to_window(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, cx| {
        let root = shell.read(cx).focus.clone();
        window.focus(&root, cx);
    }))
    .unwrap();
}

#[gpui_kit::test]
fn command_p_prints_the_page_with_the_keyboard_and_opens_go_to_file_anywhere_else(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = shell_on("command-p-prints", cx);
    let host = FakeHost::serve("command-p-prints-host", cx);
    let view = open_page_view(window, &shell, "https://example.com/", cx);
    view.update(cx, BrowserView::pretend_opened);
    let pane = view.read_with(cx, |view, _| view.pane);
    assert!(keyboard_in(window, "BrowserPage", cx), "the page has the keyboard");
    host.asked(cx);

    cx.simulate_keystrokes(window, "cmd-p");
    let print = AppRequest::Print { pane, print: Print::Preview };
    assert_eq!(
        (printed(&host, cx), windows(cx)),
        (vec![print], 1),
        "the page prints, and Go to File stays shut"
    );

    keys_to_window(window, &shell, cx);
    assert!(!keyboard_in(window, "BrowserPage", cx), "the window has the keyboard");
    cx.simulate_keystrokes(window, "cmd-p");
    assert_eq!(
        (printed(&host, cx), windows(cx)),
        (vec![], 2),
        "nothing prints, and Go to File opens over the window"
    );
}

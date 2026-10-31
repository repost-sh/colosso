//! The page's questions wait for the person: alert, confirm and prompt, and a server's
//! sign-in, each answered in the dialog the pane opens.

use std::time::Instant;

use colosso_browser_ipc::Dialog;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};

use super::{PATIENCE, finish, open, wait_for, wait_for_title};
use crate::browser::view::BrowserView;
use crate::modal;

/// Waits until the page asks a question `kind` accepts and its dialog is on screen.
fn asked(
    cx: &mut VisualTestContext,
    view: &Entity<BrowserView>,
    what: &str,
    kind: fn(&Dialog) -> bool,
) {
    wait_for(cx, view, what, |view| {
        view.asked.is_some() && view.dialogs.front().is_some_and(|q| kind(&q.asked))
    });
    let deadline = Instant::now() + PATIENCE;
    while cx.update(|_, cx| modal::window(cx)).is_none() {
        assert!(Instant::now() < deadline, "no dialog came up for {what}");
        cx.run_until_parked();
    }
}

/// Presses `before`, types `text` and presses `after` in the dialog on screen.
fn in_dialog(cx: &mut VisualTestContext, before: &str, text: &str, after: &str) {
    let window = cx
        .update(|_, cx| modal::window(cx))
        .expect("a dialog is open");
    let mut dialog = VisualTestContext::from_window(window, cx);
    dialog.update(|window, cx| window.draw(cx).clear(cx));
    dialog.run_until_parked();
    if !before.is_empty() {
        dialog.simulate_keystrokes(before);
    }
    if !text.is_empty() {
        dialog.simulate_input(text);
    }
    dialog.simulate_keystrokes(after);
    dialog.run_until_parked();
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn page_dialogs_wait_for_the_person_and_get_their_answers(cx: &mut TestAppContext) {
    let page = "data:text/html,<title>asking</title><script>setTimeout(()=>{\
        const sure = confirm('Sure?');\
        const name = prompt('Name?', 'Ada');\
        alert('Bye');\
        document.title = sure + ' ' + name;\
    }, 50)</script>";
    let (view, cx, scratch) = open("dialogs", page, cx);
    asked(cx, &view, "the confirm", |d| {
        matches!(d, Dialog::Confirm { .. })
    });
    in_dialog(cx, "", "", "enter");
    asked(cx, &view, "the prompt", |d| {
        matches!(d, Dialog::Prompt { .. })
    });
    // The field starts with the page's default; the person adds to it.
    in_dialog(cx, "end", " L", "enter");
    asked(cx, &view, "the alert", |d| {
        matches!(d, Dialog::Alert { .. })
    });
    in_dialog(cx, "", "", "escape");
    wait_for_title(cx, &view, "true Ada L");
    view.read_with(cx, |view, _| assert!(view.dialogs.is_empty()));
    finish(cx, scratch);
}

/// A local server that asks for `ada` / `pw`, then serves a page.
fn sign_in_server() -> u16 {
    super::listen(|head| {
        if head.contains("Authorization: Basic YWRhOnB3") {
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 24\r\nConnection: close\r\n\r\n<title>signed in</title>"
        } else {
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Staff\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        }
        .into()
    })
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_sign_in_dialog_sends_the_credentials_the_person_types(cx: &mut TestAppContext) {
    let port = sign_in_server();
    let (view, cx, scratch) = open("sign-in", &format!("http://127.0.0.1:{port}/"), cx);
    asked(
        cx,
        &view,
        "the sign-in",
        |d| matches!(d, Dialog::Auth { realm, proxy: false, .. } if realm == "Staff"),
    );
    in_dialog(cx, "", "ada", "tab");
    in_dialog(cx, "", "pw", "enter");
    wait_for_title(cx, &view, "signed in");
    finish(cx, scratch);
}

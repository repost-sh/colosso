//! The page's questions wait for the person: alert, confirm and prompt, and a server's
//! sign-in, each answered in the dialog the pane opens; and an agent sees the ones that hold
//! the page up, and answers them.

use std::time::Instant;

use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use koyo_app_control::ops::browser::{DialogAction, DialogKind};
use koyo_browser_ipc::Dialog;
use serde_json::json;

use super::{PATIENCE, finish, open, run, tester, wait_for, wait_for_title};
use crate::browser::agent::{self, AgentError, Answer, Unanswered};
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

/// A mouse event at the page's `(100, 50)`, as `browser_click` sends it: how the command
/// ended.
fn press(cx: &mut VisualTestContext, view: &Entity<BrowserView>, kind: &str) -> Answer {
    let event = json!({"type": kind, "x": 100, "y": 50, "button": "left", "clickCount": 1});
    run(cx, view, Some(&tester()), "Input.dispatchMouseEvent", event)
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn an_agent_sees_and_answers_the_dialogs_its_click_opens(cx: &mut TestAppContext) {
    let page = "data:text/html,<title>ready</title><body style='margin:0'>\
        <button style='position:fixed;left:0;top:0;width:200px;height:100px' \
        onclick=\"document.title=confirm('Sure?')+' '+prompt('Name?','Ada')\">go</button>";
    let (view, cx, scratch) = open("agent-dialogs", page, cx);
    wait_for(cx, &view, "the page on screen", |view| {
        view.page.title == "ready" && view.frame.is_some()
    });
    let actor = tester();

    assert!(matches!(
        press(cx, &view, "mousePressed"),
        Answer::Answered(Ok(_))
    ));
    assert_eq!(
        press(cx, &view, "mouseReleased"),
        Answer::Asking,
        "the click stops waiting as the confirm it opened holds the page up"
    );
    let asked = view
        .read_with(cx, |view, _| view.dialog())
        .expect("it asks");
    assert_eq!(
        (asked.kind, asked.message.as_str()),
        (DialogKind::Confirm, "Sure?")
    );
    let next = cx.update(|_, cx| {
        let params = json!({"expression": "1"});
        agent::cdp(&view, None, "Runtime.evaluate", params, PATIENCE, cx)
    });
    assert!(
        matches!(next, Err(AgentError::Asking(_))),
        "the page does nothing else meanwhile"
    );

    let accept = |text: Option<&str>, cx: &mut VisualTestContext| {
        let text = text.map(str::to_owned);
        view.update(cx, |view, cx| {
            view.agent_answers(&actor, DialogAction::Accept, text, cx)
        })
    };
    accept(None, cx).expect("an agent answers a confirm");
    wait_for(cx, &view, "the prompt", |view| {
        view.dialog()
            .is_some_and(|asked| asked.kind == DialogKind::Prompt)
    });
    let asked = view
        .read_with(cx, |view, _| view.dialog())
        .expect("it asks");
    assert_eq!(asked.default_text.as_deref(), Some("Ada"));
    accept(Some("Grace"), cx).expect("and a prompt, with its own text");
    wait_for_title(cx, &view, "true Grace");
    assert_eq!(accept(None, cx), Err(Unanswered::NothingAsked));
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| modal::window(cx)).is_none(),
        "the person's dialog went with the answer"
    );
    finish(cx, scratch);
}

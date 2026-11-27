//! What the person does in the app reaches the page: the keys that steer it, an address
//! entered, and a page's full screen given the screen and taken back.

use gpui_kit::TestAppContext;
use serde_json::json;

use super::super::agent::Answer;
use super::{BrowserView, address_in_page, finish, open, open_with, run, serve, wait_for_title};

/// Two pages, served locally.
fn site(name: &str) -> Option<String> {
    match name {
        "one" => Some("<title>one</title><body style='margin:0'>one".into()),
        "two" => Some("<title>two</title><body style='margin:0'>two".into()),
        _ => None,
    }
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn the_keys_take_the_page_back_and_forward(cx: &mut TestAppContext) {
    let port = serve(site);
    let (view, cx, scratch) = open("history", &format!("http://127.0.0.1:{port}/one"), cx);
    wait_for_title(cx, &view, "one");
    view.update(cx, |view, cx| {
        view.navigate(format!("http://127.0.0.1:{port}/two"), cx);
    });
    wait_for_title(cx, &view, "two");
    view.read_with(cx, |view, _| assert!(view.page.can_go_back));

    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("cmd-[");
    wait_for_title(cx, &view, "one");
    cx.simulate_keystrokes("cmd-]");
    wait_for_title(cx, &view, "two");
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn an_address_typed_into_the_bar_opens_and_the_page_takes_the_keys(cx: &mut TestAppContext) {
    let port = serve(site);
    let one = format!("http://127.0.0.1:{port}/one");
    let (view, cx, scratch) = open_with("address", &one, address_in_page, cx);
    wait_for_title(cx, &view, "one");
    cx.update(|window, cx| {
        let intent = crate::keyboard::Intent::Address;
        view.update(cx, |view, cx| view.focus_address(intent, window, cx));
    });
    cx.simulate_input(&format!("127.0.0.1:{port}/two"));
    cx.simulate_keystrokes("enter");
    wait_for_title(cx, &view, "two");
    let page_focused = cx.update(|window, cx| view.read(cx).focus.is_focused(window));
    assert!(
        page_focused,
        "the page has the keys once its address is entered"
    );
    finish(cx, scratch);
}

/// A page's full screen: the page asks for it, as a click on its button would, the app gives
/// the page the whole screen, and taking the window out of it takes the page out too, the
/// page hearing it leave. A test's window never comes forward, so its page hasn't the
/// keyboard, and Chromium turns down a full screen asked by a page without it unless a frame
/// with the person's gesture delegated it the right (`SuppressFullscreenFromUnfocusedView`):
/// the page hands its frame that right.
#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_page_full_screen_takes_the_screen_until_it_is_left(cx: &mut TestAppContext) {
    let page = "data:text/html,<title>small</title><body style='margin:0'>\
        <script>document.onfullscreenchange=()=>\
        document.title=document.fullscreenElement?'full':'small'</script>\
        <iframe allowfullscreen srcdoc='<script>onmessage=()=>\
        document.body.requestFullscreen()</script>'></iframe>";
    let (view, cx, scratch) = open("fullscreen", page, cx);
    wait_for_title(cx, &view, "small");
    let ask = json!({
        "expression": "frames[0].postMessage('', {targetOrigin: '/', delegate: 'fullscreen'})",
        "userGesture": true,
    });
    let asked = run(cx, &view, None, "Runtime.evaluate", ask);
    assert!(matches!(asked, Answer::Answered(Ok(_))), "{asked:?}");
    wait_for_title(cx, &view, "full");
    view.read_with(cx, |view, _| {
        assert!(
            view.fills_screen(),
            "on the whole screen, as the setting has it"
        );
    });
    let left = view.update(cx, BrowserView::exit_fullscreen);
    assert!(left, "it was full screen");
    wait_for_title(cx, &view, "small");
    view.read_with(cx, |view, _| assert!(!view.is_fullscreen()));
    finish(cx, scratch);
}

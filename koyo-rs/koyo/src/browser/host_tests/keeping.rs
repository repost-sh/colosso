//! What a page keeps: the engine's attention while an agent uses it out of sight, and its site
//! data until cleared. (What a page has while a pane draws it, its sight as its tab is left and
//! picked again among it, takes the app's window, which no test here has: the host's
//! `tests/embedded`.)

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use gpui_kit::{
    AppContext as _, EmptyView, Entity, TestAppContext, VisualTestContext,
};
use koyo_browser_ipc::Go;
use serde_json::{Value, json};

use super::super::agent::Answer;
use super::super::host::{Browsers, CdpResult, HostStatus};
use super::super::keys::key_codes;
use super::super::view::BrowserView;
use super::super::view::sight::AWAKE_AFTER;
use super::{finish, open, pause, prepare, run, serve, tester, wait_for_title, wait_until};

/// [`DRIVEN`], which an agent opened out of sight on an engine already running, loaded.
fn unseen<'a>(
    test: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<BrowserView>, &'a mut VisualTestContext, PathBuf) {
    let scratch = prepare(test, cx);
    let (_, cx) = cx.add_window_view(|_, _| EmptyView);
    let browsers = cx.update(|_, cx| Browsers::global(cx));
    browsers.update(cx, |browsers, cx| browsers.start_if_idle("a test", cx));
    let running = |cx: &mut VisualTestContext| {
        browsers.read_with(cx, |browsers, _| browsers.status()) == HostStatus::Running
    };
    assert!(wait_until(cx, running), "the engine never started");
    let view = cx.update(|window, cx| {
        cx.new(|cx| BrowserView::new(DRIVEN.into(), Path::new("/work/browser-test"), window, cx))
    });
    view.update(cx, BrowserView::open_out_of_sight);
    wait_for_title(cx, &view, "ready");
    (view, cx, scratch)
}

/// Runs a protocol command on the page as an agent does, acting when `acts`: its answer.
fn ask(
    cx: &mut VisualTestContext,
    view: &Entity<BrowserView>,
    acts: bool,
    method: &str,
    params: Value,
) -> CdpResult {
    let actor = acts.then(tester);
    match run(cx, view, actor.as_ref(), method, params) {
        Answer::Answered(result) => result,
        ended => panic!("{method} ended as {ended:?}"),
    }
}

/// What `expression` comes to in the page, awaited when it's a promise.
fn eval(cx: &mut VisualTestContext, view: &Entity<BrowserView>, expression: &str) -> Value {
    let params = json!({"expression": expression, "awaitPromise": true, "returnByValue": true});
    let answer = ask(cx, view, false, "Runtime.evaluate", params);
    let answer = answer.expect("the page evaluates it");
    answer.pointer("/result/value").cloned().unwrap_or_default()
}

/// Whether the page draws two animation frames, as it does on screen.
fn frames_run(cx: &mut VisualTestContext, view: &Entity<BrowserView>) -> bool {
    eval(
        cx,
        view,
        "new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(true))))",
    ) == json!(true)
}

/// A left click on [`DRIVEN`]'s button, as `browser_click` sends it, lands at once.
fn assert_click_lands(cx: &mut VisualTestContext, view: &Entity<BrowserView>) {
    let started = Instant::now();
    for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
        let event = json!({"type": kind, "x": 100, "y": 50, "button": "left", "clickCount": 1});
        ask(cx, view, true, "Input.dispatchMouseEvent", event).expect("the page takes it");
    }
    let took = started.elapsed();
    eprintln!("a click took {took:?}");
    assert_eq!(
        eval(cx, view, "[window.downs, window.clicks]"),
        json!([1, 1])
    );
    assert!(took < Duration::from_secs(1), "the click landed at once");
}

/// Enter, as `browser_press_key` sends it.
fn press_enter(cx: &mut VisualTestContext, view: &Entity<BrowserView>) {
    let (windows, native) = key_codes("enter");
    let up = json!({"type": "keyUp", "key": "Enter", "code": "Enter",
        "windowsVirtualKeyCode": windows, "nativeVirtualKeyCode": native, "modifiers": 0});
    let mut down = up.clone();
    down["type"] = json!("keyDown");
    down["text"] = json!("\r");
    down["unmodifiedText"] = json!("\r");
    for event in [down, up] {
        ask(cx, view, true, "Input.dispatchKeyEvent", event).expect("the page takes the key");
    }
}

/// Whether the page answers a screenshot with an image.
fn screenshot(cx: &mut VisualTestContext, view: &Entity<BrowserView>) -> bool {
    let started = Instant::now();
    let shot = ask(
        cx,
        view,
        false,
        "Page.captureScreenshot",
        json!({"format": "png"}),
    );
    eprintln!("a screenshot took {:?}", started.elapsed());
    shot.is_ok_and(|shot| shot["data"].as_str().is_some_and(|data| !data.is_empty()))
}

/// A page to drive: a button in its top left counting presses and clicks, a form whose
/// submit goes on at the next animation frame, as animated pages do, and its title saying
/// whether it's visible once that changes.
const DRIVEN: &str = "data:text/html,<title>ready</title><body style='margin:0'>\
    <button style='position:fixed;left:0;top:0;width:200px;height:100px' \
    onmousedown='window.downs=(window.downs||0)+1' \
    onclick='window.clicks=(window.clicks||0)+1'>go</button>\
    <form style='position:fixed;left:0;top:200px' onsubmit='event.preventDefault();\
    requestAnimationFrame(()=>document.title=\"submitted\")'><input id=name></form>\
    <script>document.addEventListener('visibilitychange',\
    ()=>document.title=document.visibilityState)</script>";

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn an_agent_drives_a_page_opened_out_of_sight_as_if_shown(cx: &mut TestAppContext) {
    let (view, cx, scratch) = unseen("unseen-driven", cx);
    assert!(frames_run(cx, &view), "animation frames run");
    assert_click_lands(cx, &view);

    eval(cx, &view, "document.getElementById('name').focus()");
    press_enter(cx, &view);
    wait_for_title(cx, &view, "submitted");
    assert!(screenshot(cx, &view), "a screenshot answers");

    cx.executor().advance_clock(AWAKE_AFTER);
    wait_for_title(cx, &view, "hidden");
    assert!(frames_run(cx, &view), "the next command wakes it");
    wait_for_title(cx, &view, "visible");
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_command_the_page_never_answers_is_answered_within_its_limit(cx: &mut TestAppContext) {
    let (view, cx, scratch) = unseen("command-limit", cx);
    cx.executor().advance_clock(AWAKE_AFTER);
    wait_for_title(cx, &view, "hidden");
    let limit = Duration::from_secs(5);
    let browsers = cx.update(|_, cx| Browsers::global(cx));
    let pane = view.read_with(cx, |view, _| view.pane);
    let mut asked = browsers.update(cx, |browsers, cx| {
        let params = json!({"expression": "new Promise(() => {})", "awaitPromise": true});
        browsers.ask(pane, None, "Runtime.evaluate", params, limit, cx)
    });
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(1) {
        cx.run_until_parked();
        pause(Duration::from_millis(20));
    }
    assert_eq!((&mut asked).now_or_never(), None, "nothing answers it");

    cx.executor().advance_clock(limit);
    cx.run_until_parked();
    assert_eq!((&mut asked).now_or_never(), Some(Answer::TimedOut));
    let waiting = browsers.read_with(cx, |browsers, _| browsers.calls_waiting());
    assert_eq!(waiting, 0, "the app forgot the command");
    finish(cx, scratch);
}

/// A page saying whether the site kept what its last visit stored.
fn store(name: &str) -> Option<String> {
    (name == "store").then(|| {
        "<script>const kept=localStorage.getItem('visited');\
         localStorage.setItem('visited','yes');document.title=kept?'kept':'fresh'</script>"
            .to_owned()
    })
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn clearing_browsing_data_opens_the_page_again_with_nothing_kept(cx: &mut TestAppContext) {
    let port = serve(store);
    let (view, cx, scratch) = open("clear", &format!("http://127.0.0.1:{port}/store"), cx);
    wait_for_title(cx, &view, "fresh");
    view.update(cx, |view, cx| view.go(Go::Reload, cx));
    wait_for_title(cx, &view, "kept");

    cx.update(|_, cx| crate::browser::clear_data(cx));
    wait_for_title(cx, &view, "fresh");
    view.read_with(cx, |view, cx| {
        assert_eq!(view.status(cx), HostStatus::Running);
    });
    finish(cx, scratch);
}

/// A page that signs a person in as sites do, and whose title says what its site still had of
/// an earlier visit: a session cookie, a cookie with an age, and what it kept in its storage.
fn account(name: &str) -> Option<String> {
    (name == "account").then(|| {
        "<script>const had=[document.cookie.includes('session=1')&&'session',\
         document.cookie.includes('kept=1')&&'cookie',\
         localStorage.getItem('token')&&'storage'].filter(Boolean).join('+');\
         document.cookie='session=1; path=/';\
         document.cookie='kept=1; path=/; max-age=86400';\
         localStorage.setItem('token','yes');\
         document.title=had||'signed out'</script>"
            .to_owned()
    })
}

/// What a pinned page counts on (`docs/arc-sidebar.md` 11): its project's profile keeps a
/// sign-in across Koyo quitting and starting again, when the engine goes and comes back.
#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_sign_in_outlives_the_engine_quitting_and_starting_again(cx: &mut TestAppContext) {
    let port = serve(account);
    let (view, cx, scratch) = open("signed-in", &format!("http://127.0.0.1:{port}/account"), cx);
    wait_for_title(cx, &view, "signed out");

    let browsers = cx.update(|_, cx| Browsers::global(cx));
    let connection = browsers.update(cx, |browsers, _| browsers.stop());
    connection
        .expect("the engine, running")
        .shutdown(Duration::from_secs(5));
    browsers.update(cx, |browsers, cx| browsers.start_if_idle("a test", cx));
    wait_for_title(cx, &view, "session+cookie+storage");
    finish(cx, scratch);
}

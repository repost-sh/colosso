//! What a page keeps: its paint while out of sight, the engine's attention while an agent
//! uses it out of sight, and its site data until cleared.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};
use koyo_browser_ipc::Go;
use serde_json::{Value, json};

use super::super::agent::Answer;
use super::super::host::{Browsers, CdpResult, HostStatus};
use super::super::keys::key_codes;
use super::super::view::BrowserView;
use super::super::view::sight::AWAKE_FOR;
use super::{
    centre, finish, open, pause, prepare, run, serve, tester, wait_for, wait_for_title, wait_until,
};

/// A window showing its page or not, as a tab does once another is picked.
struct Tab {
    page: Entity<BrowserView>,
    shown: bool,
}

impl Render for Tab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .children(self.shown.then(|| self.page.clone()))
    }
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_page_out_of_sight_is_hidden_and_paints_again_once_back(cx: &mut TestAppContext) {
    let url = "data:text/html,<title>visible</title><body style='margin:0;background:%230f0'>\
        <script>document.addEventListener('visibilitychange',\
        ()=>document.title=document.visibilityState)</script>";
    let scratch = prepare("sight", cx);
    let (tab, cx) = cx.add_window_view(|window, cx| {
        let page =
            cx.new(|cx| BrowserView::new(url.into(), Path::new("/work/browser-test"), window, cx));
        Tab { page, shown: true }
    });
    let view = tab.read_with(cx, |tab, _| tab.page.clone());
    wait_for(cx, &view, "the page on screen", |view| {
        view.page.title == "visible" && view.frame.is_some()
    });

    show(&tab, false, cx);
    wait_for_title(cx, &view, "hidden");
    let painted = view.read_with(cx, |view, _| view.frames_shown);

    show(&tab, true, cx);
    wait_for(
        cx,
        &view,
        "the page back on screen and painted afresh",
        |view| view.page.title == "visible" && view.frames_shown > painted,
    );
    finish(cx, scratch);
}

/// A window whose tab doesn't show `url`'s page, on an engine already running, as an agent's
/// page opened out of sight finds it.
fn unseen<'a>(
    test: &str,
    url: &str,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Tab>,
    Entity<BrowserView>,
    &'a mut VisualTestContext,
    PathBuf,
) {
    let scratch = prepare(test, cx);
    let url = url.to_owned();
    let (tab, cx) = cx.add_window_view(|window, cx| {
        let page = cx.new(|cx| BrowserView::new(url, Path::new("/work/browser-test"), window, cx));
        Tab { page, shown: false }
    });
    let browsers = cx.update(|_, cx| Browsers::global(cx));
    browsers.update(cx, Browsers::start_if_idle);
    let running = |cx: &mut VisualTestContext| {
        browsers.read_with(cx, |browsers, _| browsers.status()) == HostStatus::Running
    };
    assert!(wait_until(cx, running), "the engine never started");
    let view = tab.read_with(cx, |tab, _| tab.page.clone());
    (tab, view, cx, scratch)
}

/// [`DRIVEN`] shown, then its tab left, as a tab is once another is picked.
fn left<'a>(
    test: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<BrowserView>, &'a mut VisualTestContext, PathBuf) {
    let (tab, view, cx, scratch) = unseen(test, DRIVEN, cx);
    show(&tab, true, cx);
    wait_for(cx, &view, "the page on screen", |view| {
        view.page.title == "ready" && view.frame.is_some()
    });
    show(&tab, false, cx);
    wait_for_title(cx, &view, "hidden");
    (view, cx, scratch)
}

/// Shows the tab, or puts it out of sight.
fn show(tab: &Entity<Tab>, shown: bool, cx: &mut VisualTestContext) {
    tab.update(cx, |tab, cx| {
        tab.shown = shown;
        cx.notify();
    });
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
fn a_page_opened_out_of_sight_paints_once_shown(cx: &mut TestAppContext) {
    let url = "data:text/html,<body style='margin:0;background:%230f0'><script>\
        const say=()=>document.title=document.visibilityState;say();\
        document.addEventListener('visibilitychange',say)</script>";
    let (tab, view, cx, scratch) = unseen("unseen-paints", url, cx);
    view.update(cx, BrowserView::open_out_of_sight);
    // No agent uses it, so the engine has it out of sight, likely before its browser is
    // made, and Chromium is told so all the same.
    cx.executor().advance_clock(AWAKE_FOR);
    wait_for_title(cx, &view, "hidden");
    show(&tab, true, cx);
    wait_for(cx, &view, "the page painted once shown", |view| {
        view.page.title == "visible" && view.frame.as_ref().is_some_and(|f| centre(f)[1] > 240)
    });
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn an_agent_drives_a_page_opened_out_of_sight_as_if_shown(cx: &mut TestAppContext) {
    let (_tab, view, cx, scratch) = unseen("unseen-driven", DRIVEN, cx);
    view.update(cx, BrowserView::open_out_of_sight);
    wait_for_title(cx, &view, "ready");
    assert!(frames_run(cx, &view), "animation frames run");
    assert_click_lands(cx, &view);

    eval(cx, &view, "document.getElementById('name').focus()");
    press_enter(cx, &view);
    wait_for_title(cx, &view, "submitted");
    assert!(screenshot(cx, &view), "a screenshot answers");

    // Unused for a while, it's out of sight for the engine too, until the next command.
    cx.executor().advance_clock(AWAKE_FOR);
    wait_for_title(cx, &view, "hidden");
    assert!(frames_run(cx, &view), "the next command wakes it");
    wait_for_title(cx, &view, "visible");
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn an_agent_drives_a_page_whose_tab_was_left(cx: &mut TestAppContext) {
    let (view, cx, scratch) = left("left-driven", cx);
    assert!(screenshot(cx, &view), "a screenshot answers");
    wait_for_title(cx, &view, "visible");
    assert!(frames_run(cx, &view), "animation frames run");
    assert_click_lands(cx, &view);
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_capture_that_cannot_complete_answers_within_its_limit(cx: &mut TestAppContext) {
    let (view, cx, scratch) = left("capture-limit", cx);
    // Asked of the engine straight, which no agent's command is, so the page stays hidden
    // and has no frame to capture.
    let limit = Duration::from_secs(5);
    let browsers = cx.update(|_, cx| Browsers::global(cx));
    let pane = view.read_with(cx, |view, _| view.pane);
    let mut capture = browsers.update(cx, |browsers, cx| {
        let params = json!({"format": "png"});
        browsers.ask(pane, "Page.captureScreenshot", params, limit, cx)
    });
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(1) {
        cx.run_until_parked();
        pause(Duration::from_millis(20));
    }
    assert_eq!(
        (&mut capture).now_or_never(),
        None,
        "a hidden page captures nothing"
    );

    cx.executor().advance_clock(limit);
    cx.run_until_parked();
    assert_eq!((&mut capture).now_or_never(), Some(Answer::TimedOut));
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
fn clearing_browsing_data_restarts_the_engine_with_nothing_kept(cx: &mut TestAppContext) {
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

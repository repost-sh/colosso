//! A page that takes a while says how far it got, and one whose renderer went away loads
//! again from the notice over it.

use std::time::Duration;

use koyo_browser_ipc::Go;
use serde_json::json;

use super::{
    PATIENCE, address_in_page, finish, open, open_with, pause, run, serve, wait_for,
    wait_for_title,
};
use crate::browser::agent;
use crate::browser::view::Notice;

/// A page whose picture comes a second and a half late, so it loads for that long.
fn slow(name: &str) -> Option<String> {
    match name {
        "slow" => Some("<title>slow</title><img src='/late.png'>".to_owned()),
        "late.png" => {
            pause(Duration::from_millis(1500));
            Some(String::new())
        }
        _ => None,
    }
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_loading_page_shows_how_far_it_got_until_it_is_done(cx: &mut gpui_kit::TestAppContext) {
    let port = serve(slow);
    let address = format!("http://127.0.0.1:{port}/slow");
    let (view, cx, scratch) = open_with("progress", &address, address_in_page, cx);
    wait_for(cx, &view, "the page partly loaded", |view| {
        view.page.loading && view.progress > 0 && view.progress < 100
    });
    assert!(
        cx.debug_bounds("browser-progress").is_some(),
        "the toolbar shows the load"
    );
    wait_for(cx, &view, "the page loaded", |view| {
        view.page.title == "slow" && !view.page.loading
    });
    assert!(
        cx.debug_bounds("browser-progress").is_none(),
        "the line goes once it's loaded"
    );
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_page_whose_renderer_crashed_loads_again_from_the_notices_reload(
    cx: &mut gpui_kit::TestAppContext,
) {
    let port = serve(|name| (name == "page").then(|| "<title>page</title>here".to_owned()));
    let (view, cx, scratch) = open("crashed", &format!("http://127.0.0.1:{port}/page"), cx);
    wait_for(cx, &view, "the page loaded", |view| {
        view.page.title == "page" && !view.page.loading
    });
    run(
        cx,
        &view,
        None,
        "Runtime.evaluate",
        json!({"expression": "document.title = 'touched'"}),
    );
    wait_for_title(cx, &view, "touched");
    let crash =
        cx.update(|_, cx| agent::cdp(&view, None, None, "Page.crash", json!({}), PATIENCE, cx));
    crash.expect("the page takes commands").detach();
    wait_for(cx, &view, "the notice that the page went away", |view| {
        matches!(view.notice, Some(Notice::Gone(_)))
    });
    view.update(cx, |view, cx| view.go(Go::Reload, cx));
    wait_for(
        cx,
        &view,
        "the page loaded again in a renderer of its own",
        |view| view.notice.is_none() && view.page.title == "page" && !view.page.loading,
    );
    let answer = run(
        cx,
        &view,
        None,
        "Runtime.evaluate",
        json!({"expression": "document.title"}),
    );
    assert!(
        matches!(&answer, agent::Answer::Answered(Ok(result)) if result["result"]["value"] == "page"),
        "the page answers again: {answer:?}"
    );
    finish(cx, scratch);
}

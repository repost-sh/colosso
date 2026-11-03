//! A page that takes a while says how far it got.

use std::time::Duration;

use super::{finish, open, pause, serve, wait_for};

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
    let (view, cx, scratch) = open("progress", &format!("http://127.0.0.1:{port}/slow"), cx);
    wait_for(cx, &view, "the page partly loaded", |view| {
        view.page.loading && view.page.progress > 0 && view.page.progress < 100
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

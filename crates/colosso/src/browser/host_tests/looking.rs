//! Looking at a page: the find bar counts the matches of what's typed in it, and zooming
//! lays the page out in less or more room.

use gpui_kit::TestAppContext;

use super::{finish, open, wait_for};

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn find_counts_the_matches_and_zooming_lays_the_page_out_afresh(cx: &mut TestAppContext) {
    let page = "data:text/html,<p>needle one</p><p>needle two</p><p>needle three</p>\
        <script>const say=()=>document.title='w'+innerWidth;onresize=say;say()</script>";
    let (view, cx, scratch) = open("looking", page, cx);
    wait_for(cx, &view, "the page's width", |view| {
        view.page.title.starts_with('w')
    });
    let width = |view: &super::BrowserView| {
        view.page
            .title
            .trim_start_matches('w')
            .parse::<f32>()
            .unwrap_or(0.)
    };
    let before = view.read_with(cx, |view, _| width(view));
    assert!(before > 100., "the page is {before} wide");

    cx.update(|window, cx| view.update(cx, |view, cx| view.open_find(window, cx)));
    cx.run_until_parked();
    cx.simulate_input("needle");
    let typed = view.read_with(cx, super::BrowserView::find_text);
    assert_eq!(
        typed.as_deref(),
        Some("needle"),
        "the find bar takes the keys"
    );
    wait_for(cx, &view, "three matches", |view| {
        view.find_tally() == Some((3, 1))
    });
    cx.simulate_keystrokes("enter");
    wait_for(cx, &view, "the second one", |view| {
        view.find_tally() == Some((3, 2))
    });
    cx.simulate_keystrokes("shift-enter");
    wait_for(cx, &view, "the first again", |view| {
        view.find_tally() == Some((3, 1))
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.find_tally().is_none(), "escape closes it");
    });

    view.update(cx, |view, cx| view.set_zoom(200, cx));
    wait_for(cx, &view, "half the room", |view| {
        (width(view) - before / 2.).abs() <= 1.
    });
    view.update(cx, |view, cx| view.zoom_by(-1, cx));
    view.read_with(cx, |view, _| assert_eq!(view.zoom, 175));
    finish(cx, scratch);
}

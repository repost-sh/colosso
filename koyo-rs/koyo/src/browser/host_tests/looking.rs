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

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn an_element_is_inspected_again_while_the_tools_are_open(cx: &mut TestAppContext) {
    let (view, cx, scratch) = open("inspecting", "data:text/html,<title>loaded</title><p>hi", cx);
    super::wait_for_title(cx, &view, "loaded");
    let at = koyo_browser_ipc::Point { x: 10., y: 10. };
    view.update(cx, |view, cx| view.inspect(Some(at), cx));
    let open = super::wait_until(cx, |cx| {
        let tools = view.read_with(cx, |view, _| view.devtools().cloned());
        tools.is_some_and(|tools| tools.read_with(cx, super::BrowserView::is_open))
    });
    assert!(open, "the tools open");
    view.update(cx, |view, cx| view.inspect(Some(at), cx));
    view.read_with(cx, |view, _| assert!(view.devtools().is_some(), "still open"));
    finish(cx, scratch);
}

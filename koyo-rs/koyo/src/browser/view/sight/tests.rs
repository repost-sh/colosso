//! A page out of sight is kept on screen for the engine while an agent's command on it runs,
//! and for a few seconds after, then rests; a tab the person leaves or comes to tells the host
//! nothing, which knows itself that a pane stopped or started drawing the page.

use std::path::Path;
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton, TestAppContext,
    VisualTestContext, px, size,
};
use koyo_browser_ipc::{AppRequest, HostEvent, PaneId};
use serde_json::json;

use super::AWAKE_AFTER;
use crate::browser::agent;
use crate::browser::host::fake::FakeHost;
use crate::browser::view::BrowserView;
use crate::settings::Layout;
use crate::test_support::{
    center_of, click_pane, draw, hover, in_front, in_window, on_screen, open_page_view, shell_on,
};

/// How long a command waits for the page in these tests.
const LIMIT: Duration = Duration::from_secs(30);

/// A page the fake host opened, drawn once, in a window the person then stops seeing.
fn out_of_sight(name: &str, cx: &mut TestAppContext) -> (FakeHost, AnyWindowHandle, Entity<BrowserView>) {
    cx.update(crate::test_support::init);
    let host = FakeHost::serve(name, cx);
    let (window, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
        cx.new(|cx| BrowserView::new("https://a.example/".into(), Path::new("/work/app"), window, cx))
    });
    draw(window, cx);
    on_screen(window, false, cx);
    (host, window, view)
}

/// What the host was told of `pane`'s page's sight, in order: whether an agent keeps it, and
/// whether its window is seen.
fn told(asked: &[AppRequest], pane: PaneId) -> Vec<(bool, bool)> {
    (asked.iter())
        .filter_map(|asked| match asked {
            AppRequest::Sight { pane: of, kept, seen } if *of == pane => Some((*kept, *seen)),
            _ => None,
        })
        .collect()
}

/// Out of sight, kept by an agent or not.
const KEPT: (bool, bool) = (true, false);
const RESTS: (bool, bool) = (false, false);

/// The number of the agent command on `pane` among `asked`.
fn command(asked: &[AppRequest], pane: PaneId) -> u64 {
    (asked.iter())
        .find_map(|asked| match asked {
            AppRequest::Cdp { pane: of, call, .. } if *of == pane => Some(*call),
            _ => None,
        })
        .expect("the command reaches the host")
}

#[gpui_kit::test]
fn a_page_out_of_sight_is_awake_while_an_agents_command_runs_and_rests_soon_after(
    cx: &mut TestAppContext,
) {
    let (host, _window, view) = out_of_sight("awake-command", cx);
    let pane = view.read_with(cx, |view, _| view.pane);
    assert_eq!(told(&host.asked(cx), pane).last(), Some(&RESTS), "out of sight, it rests");

    let params = json!({ "expression": "1" });
    let answer = cx.update(|cx| agent::cdp(&view, None, None, "Runtime.evaluate", params, LIMIT, cx));
    let _answer = answer.expect("the page takes commands");
    let asked = host.asked(cx);
    let call = command(&asked, pane);
    assert_eq!(told(&asked, pane), [KEPT], "woken before the command: {asked:?}");

    cx.executor().advance_clock(LIMIT.saturating_sub(Duration::from_secs(5)));
    assert_eq!(told(&host.asked(cx), pane), [], "awake while the command runs");

    let reply = HostEvent::CdpReply {
        pane,
        call,
        result: Some(json!({ "result": { "value": 1 } })),
        error: None,
    };
    host.says(reply, cx);
    cx.executor().advance_clock(AWAKE_AFTER.saturating_sub(Duration::from_secs(1)));
    assert_eq!(told(&host.asked(cx), pane), [], "awake a moment after the answer");
    cx.executor().advance_clock(Duration::from_secs(2));
    assert_eq!(told(&host.asked(cx), pane), [RESTS], "rests a few seconds after the answer");
}

#[gpui_kit::test]
fn a_command_given_up_on_keeps_its_page_awake_no_longer_than_it_could_have_run(
    cx: &mut TestAppContext,
) {
    let (host, _window, view) = out_of_sight("awake-dropped", cx);
    let pane = view.read_with(cx, |view, _| view.pane);
    host.asked(cx);
    let params = json!({ "expression": "1" });
    let answer = cx.update(|cx| agent::cdp(&view, None, None, "Runtime.evaluate", params, LIMIT, cx));
    drop(answer.expect("the page takes commands"));
    assert_eq!(told(&host.asked(cx), pane), [KEPT]);
    cx.executor().advance_clock(LIMIT + AWAKE_AFTER + Duration::from_secs(1));
    assert_eq!(told(&host.asked(cx), pane), [RESTS], "it rests once the command could be over");
}

#[gpui_kit::test]
fn a_page_the_person_sees_stays_on_screen_after_an_agents_command(cx: &mut TestAppContext) {
    let (host, window, view) = out_of_sight("awake-seen", cx);
    let pane = view.read_with(cx, |view, _| view.pane);
    on_screen(window, true, cx);
    draw(window, cx);
    host.asked(cx);
    let params = json!({ "expression": "1" });
    let answer = cx.update(|cx| agent::cdp(&view, None, None, "Runtime.evaluate", params, LIMIT, cx));
    drop(answer.expect("the page takes commands"));
    cx.executor().advance_clock(LIMIT + AWAKE_AFTER + Duration::from_secs(1));
    let told = told(&host.asked(cx), pane);
    assert!(told.iter().all(|&(_, seen)| seen), "shown, its window is seen throughout: {told:?}");
}

/// Two pages in tabs of their own, the second shown, its page with the keyboard: the window,
/// the host and the two pages' panes.
fn two_tabs(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, FakeHost, PaneId, PaneId) {
    let (window, shell, _) = shell_on(name, Layout::Orca, cx);
    let host = FakeHost::serve(&format!("{name}-host"), cx);
    let first = open_page_view(window, &shell, "https://a.example/", cx);
    let second = open_page_view(window, &shell, "https://b.example/", cx);
    in_front(window, true, cx);
    let shown = second.read_with(cx, |view, _| view.pane);
    click_pane(window, shell.read_with(cx, |shell, _| shell.shown_tab_key()).expect("a tab shown"), cx);
    host.asked(cx);
    (window, host, first.read_with(cx, |view, _| view.pane), shown)
}

#[gpui_kit::test]
fn a_tab_the_person_leaves_for_another_tells_the_host_nothing_of_either_page(cx: &mut TestAppContext) {
    let (window, host, first, left) = two_tabs("sight-leave", cx);
    let tab = center_of(window, "tab-0-0-0", cx);
    hover(window, tab, cx);
    draw(window, cx);
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.simulate_mouse_down(tab, MouseButton::Left, Modifiers::default());
    draw(window, cx);
    let pressed = host.asked(cx);
    assert_eq!(told(&pressed, left), [], "a press on a tab leaves nothing yet: {pressed:?}");
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.simulate_mouse_up(tab, MouseButton::Left, Modifiers::default());
    for _ in 0..3 {
        draw(window, cx);
    }
    let asked = host.asked(cx);
    assert_eq!(told(&asked, left), [], "the host knows the page left isn't drawn: {asked:?}");
    assert_eq!(told(&asked, first), [], "nor that the page come to is: {asked:?}");
}

#[gpui_kit::test]
fn a_tab_left_by_a_press_and_release_at_once_tells_the_host_nothing_of_either_page(
    cx: &mut TestAppContext,
) {
    let (window, host, first, left) = two_tabs("sight-leave-at-once", cx);
    let tab = center_of(window, "tab-0-0-0", cx);
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.simulate_mouse_move(tab, None, Modifiers::default());
    visual.simulate_click(tab, Modifiers::default());
    for _ in 0..3 {
        draw(window, cx);
    }
    let asked = host.asked(cx);
    assert_eq!(told(&asked, left), [], "the host knows the page left isn't drawn: {asked:?}");
    assert_eq!(told(&asked, first), [], "nor that the page come to is: {asked:?}");
}

/// A page drawn while its window counted as unseen tells the host, at the next paint, once the
/// window counts as seen, whatever made it so: what it last told is checked against what it
/// would tell at every paint, not only as its pane or its window's visibility changes. Told
/// only then, a page that counted as seen with no callback stayed hidden for the engine while
/// it was drawn, and took no focus.
#[gpui_kit::test]
fn a_drawn_page_tells_the_host_its_window_is_seen_at_the_next_paint(cx: &mut TestAppContext) {
    let (host, window, view) = out_of_sight("seen-at-paint", cx);
    let pane = view.read_with(cx, |view, _| view.pane);
    assert_eq!(told(&host.asked(cx), pane).last(), Some(&RESTS), "drawn unseen, it rests");

    crate::sight::count_unseen(true);
    view.update(cx, |_, cx| cx.notify());
    draw(window, cx);
    crate::sight::count_unseen(false);
    assert_eq!(told(&host.asked(cx), pane), [(false, true)], "seen, it's told so as it's drawn");
}

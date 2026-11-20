//! A page out of sight is on screen for the engine while an agent's command on it runs, and
//! for a few seconds after, then rests.

use std::path::Path;
use std::time::Duration;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, px, size};
use koyo_browser_ipc::{AppRequest, HostEvent, PaneId};
use serde_json::json;

use super::AWAKE_AFTER;
use crate::browser::agent;
use crate::browser::host::fake::FakeHost;
use crate::browser::view::BrowserView;
use crate::test_support::{draw, in_window, on_screen};

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

/// What the host was told of whether `pane`'s page is on screen, in order.
fn told(asked: &[AppRequest], pane: PaneId) -> Vec<bool> {
    (asked.iter())
        .filter_map(|asked| match asked {
            AppRequest::Visible { pane: of, visible } if *of == pane => Some(*visible),
            _ => None,
        })
        .collect()
}

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
    assert_eq!(told(&host.asked(cx), pane).last(), Some(&false), "out of sight, it rests");

    let params = json!({ "expression": "1" });
    let answer = cx.update(|cx| agent::cdp(&view, None, None, "Runtime.evaluate", params, LIMIT, cx));
    let _answer = answer.expect("the page takes commands");
    let asked = host.asked(cx);
    let call = command(&asked, pane);
    assert_eq!(told(&asked, pane), [true], "woken before the command: {asked:?}");

    cx.executor().advance_clock(LIMIT.saturating_sub(Duration::from_secs(5)));
    assert_eq!(told(&host.asked(cx), pane), [false; 0], "awake while the command runs");

    let reply = HostEvent::CdpReply {
        pane,
        call,
        result: Some(json!({ "result": { "value": 1 } })),
        error: None,
    };
    host.says(reply, cx);
    cx.executor().advance_clock(AWAKE_AFTER.saturating_sub(Duration::from_secs(1)));
    assert_eq!(told(&host.asked(cx), pane), [false; 0], "awake a moment after the answer");
    cx.executor().advance_clock(Duration::from_secs(2));
    assert_eq!(told(&host.asked(cx), pane), [false], "rests a few seconds after the answer");
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
    assert_eq!(told(&host.asked(cx), pane), [true]);
    cx.executor().advance_clock(LIMIT + AWAKE_AFTER + Duration::from_secs(1));
    assert_eq!(told(&host.asked(cx), pane), [false], "it rests once the command could be over");
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
    assert_eq!(told(&host.asked(cx), pane), [false; 0], "shown, it never rests");
}

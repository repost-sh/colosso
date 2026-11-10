//! A tab's row says how the tab's doing (`docs/arc-sidebar.md` 5), as its project's tile does:
//! the spinner while a chat in it is at work, an indigo dot once something in it waits for the
//! person, at the row's end, where its × is.

use gpui_kit::{Bounds, Pixels, TestAppContext, VisualTestContext};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::model::Status;
use koyo_agent::protocol::requests::RequestKind;

use super::{arc_shell, chat, hear};
use crate::chat::Activity;
use crate::model::{Edge, Pane, PaneKind, Tab};
use crate::test_support::draw;

/// Where `selector` names something drawn, if it names anything.
fn drawn(
    window: gpui_kit::AnyWindowHandle,
    selector: String,
    cx: &mut TestAppContext,
) -> Option<Bounds<Pixels>> {
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    VisualTestContext::from_window(window, cx).debug_bounds(selector)
}

#[gpui_kit::test]
fn a_tabs_row_says_its_chat_works_and_then_that_it_waits(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["arc-tab-says"], cx);
    // The chat's tab isn't the one shown, so nothing marks it seen.
    let pane = shell.update(cx, |shell, cx| {
        let mut pane = Pane::new(PaneKind::Chat);
        pane.title = "A chat".into();
        pane.chat = Some(ThreadId("0001".into()));
        let id = pane.id;
        shell.workspace.push_tab(Tab::Single(pane));
        shell.workspace.push_tab(chat("another"));
        cx.notify();
        id
    });
    let worktree = roots[0].clone();
    let hear = |status, needs, unread, cx: &mut TestAppContext| {
        hear(&shell, ("0001", &worktree), (status, needs, unread), cx);
        draw(window, cx);
    };
    // What the tab's doing, and where its row says so: with the spinner, or with the dot.
    let told = |cx: &mut TestAppContext| {
        draw(window, cx);
        let activity = cx.read(|cx| {
            let shell = shell.read(cx);
            shell.tab_activity(&shell.workspace.branch().unwrap().tabs[0], cx)
        });
        let working = drawn(window, format!("arc-tab-working-{pane}"), cx);
        let waiting = drawn(window, format!("arc-tab-waiting-{pane}"), cx);
        (activity, working, waiting)
    };
    assert_eq!(told(cx), (Activity::Quiet, None, None));

    hear(Status::Working, None, false, cx);
    let (activity, working, waiting) = told(cx);
    assert_eq!((activity, waiting), (Activity::Working, None));
    // At the row's end, in the middle of its height.
    let spinner = working.expect("the spinner on the row of the chat at work");
    let row = drawn(window, "tab-0-0-0".into(), cx).expect("the chat's row");
    assert!(
        row.right() - spinner.right() < gpui_kit::px(16.),
        "{spinner:?} in {row:?}"
    );
    assert!(spinner.left() > row.center().x);
    assert_eq!(spinner.center().y, row.center().y);

    // Finished and not seen: the dot, where the spinner was.
    hear(Status::Idle, None, true, cx);
    let (activity, working, waiting) = told(cx);
    assert_eq!((activity, working), (Activity::Attention, None));
    let dot = waiting.expect("the dot on the row of the chat that finished");
    assert_eq!(dot.center(), spinner.center());

    // Asking the person while it works is waiting for them.
    hear(Status::Working, Some(RequestKind::Approval), false, cx);
    assert_eq!(told(cx).0, Activity::Attention);
    hear(Status::Idle, None, false, cx);
    assert_eq!(told(cx), (Activity::Quiet, None, None), "seen: nothing");

    // In a split, whose row is its panes, it goes before them.
    hear(Status::Working, None, false, cx);
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout");
        let beside = unit.tabs[0].first().id;
        assert!(
            unit.drop_tab(1, beside, Edge::Right),
            "the two chats side by side"
        );
        cx.notify();
    });
    let (activity, working, _) = told(cx);
    assert_eq!(activity, Activity::Working);
    let spinner = working.expect("the spinner on the split's row");
    let row = drawn(window, "tab-0-0-0".into(), cx).expect("the split's row");
    assert!(spinner.right() < row.center().x, "{spinner:?} in {row:?}");
}

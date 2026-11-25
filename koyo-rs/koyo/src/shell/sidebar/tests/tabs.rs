//! A tab's row in the sidebar says what goes on in the tab (`docs/activity.md`) at the row's end,
//! where its × is: the spinner for its work, and an indigo dot once something in it waits for the
//! person.

use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, point, px};
use koyo_agent::protocol::model::{Beside, Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;

use super::{drawn, hear, sidebar_shell, unseen_chat};
use crate::model::Edge;
use crate::test_support::{click, draw, hover};

#[gpui_kit::test]
fn a_tabs_row_says_its_chat_works_and_then_that_it_waits(cx: &mut TestAppContext) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-tab-says"], cx);
    let pane = unseen_chat(&shell, cx);
    let worktree = roots[0].clone();
    let hear = |status, needs, unread, cx: &mut TestAppContext| {
        hear(&shell, ("0001", &worktree), (status, needs, unread), cx);
        draw(window, cx);
    };
    let told = |cx: &mut TestAppContext| {
        draw(window, cx);
        let spinner = drawn(window, format!("sidebar-tab-{pane}-spinner"), cx);
        let dot = drawn(window, format!("sidebar-tab-{pane}-dot"), cx);
        (spinner, dot)
    };
    assert_eq!(told(cx), (None, None));

    hear(Status::Working, None, false, cx);
    let (working, waiting) = told(cx);
    assert_eq!(waiting, None);
    let spinner = working.expect("the spinner on the row of the chat at work");
    let row = drawn(window, "tab-0-0-0", cx).expect("the chat's row");
    assert!(
        row.right() - spinner.right() < px(16.),
        "{spinner:?} in {row:?}"
    );
    assert!(spinner.left() > row.center().x);
    assert_eq!(spinner.center().y, row.center().y);

    hear(Status::Idle, None, true, cx);
    let (working, waiting) = told(cx);
    assert_eq!(working, None);
    let dot = waiting.expect("the dot on the row of the chat that finished");
    assert_eq!(dot.center(), spinner.center());

    hear(Status::Working, None, true, cx);
    let (working, waiting) = told(cx);
    let (both, beside) = (working.expect("the spinner too"), waiting.expect("with the dot"));
    assert_eq!(both, spinner, "the spinner keeps its place in the column");
    assert!(beside.right() < both.left(), "the dot beside it: {beside:?} and {both:?}");
    assert_eq!(beside.center().y, both.center().y);

    hear(Status::Working, Some(RequestKind::Approval), false, cx);
    assert!(matches!(told(cx), (None, Some(_))), "asking isn't work");
    hear(Status::Idle, None, false, cx);
    assert_eq!(told(cx), (None, None), "seen: nothing");

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
    let (working, _) = told(cx);
    let spinner = working.expect("the spinner on the split's row");
    let row = drawn(window, "tab-0-0-0", cx).expect("the split's row");
    assert!(spinner.right() < row.center().x, "{spinner:?} in {row:?}");
}

#[gpui_kit::test]
fn a_chats_row_says_what_was_said_last_unless_its_tab_is_shown(cx: &mut TestAppContext) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-tab-said"], cx);
    let pane = unseen_chat(&shell, cx);
    let summary = ThreadSummary {
        said: Some("It shares a folder.".into()),
        ..crate::chat::tests::summary("0001", &roots[0], "A chat", true)
    };
    crate::chat::tests::tell(&shell, summary, cx);
    draw(window, cx);
    let said = format!("sidebar-tab-said-{pane}");
    assert!(
        drawn(window, said.clone(), cx).is_some(),
        "under its title, its tab not shown"
    );
    shell.update(cx, |shell, cx| {
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    draw(window, cx);
    assert!(
        drawn(window, said, cx).is_none(),
        "shown, its chat says it beside the sidebar"
    );
}

#[gpui_kit::test]
fn a_tabs_title_runs_on_to_its_rows_end_until_the_pointers_on_it(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-tab-title"], cx);
    let pane = unseen_chat(&shell, cx);
    draw(window, cx);
    let title = format!("sidebar-tab-title-{pane}");
    let row = drawn(window, "tab-0-0-0", cx).expect("the chat's row");
    let alone = drawn(window, title.clone(), cx).expect("its title");
    assert!(
        row.right() - alone.right() <= px(6.),
        "nothing at its end, the title runs on to it: {alone:?} in {row:?}"
    );
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_move(row.center(), None, Modifiers::default());
    draw(window, cx);
    let hovered = drawn(window, title, cx).expect("its title, hovered");
    assert!(
        hovered.right() <= alone.right() - px(24.),
        "hovered, it makes room for the ×: {hovered:?}, from {alone:?}"
    );
}

#[gpui_kit::test]
fn a_splits_pane_shows_its_x_while_the_pointers_on_it(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-split-close"], cx);
    unseen_chat(&shell, cx);
    let right = shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout");
        let (beside, right) = (unit.tabs[0].first().id, unit.tabs[1].first().id);
        assert!(
            unit.drop_tab(1, beside, Edge::Right),
            "the two chats side by side"
        );
        cx.notify();
        right
    });
    draw(window, cx);
    let row = drawn(window, "tab-0-0-0", cx).expect("the split's row");
    let on = point(row.left() + row.size.width * 0.75, row.center().y);
    VisualTestContext::from_window(window, cx).simulate_mouse_move(on, None, Modifiers::default());
    draw(window, cx);
    let close = drawn(window, format!("pane-chip-close-{right}"), cx).expect("its ×");
    assert!(
        close.left() > row.center().x && close.right() <= row.right() - px(6.),
        "its × shows at its end, in the row: {close:?} in {row:?}"
    );
}

/// What's going on in a tab is said once, on the last row in sight over it: the tab's while it's
/// listed, its branch's once that's collapsed, its project's once that's folded; each in the one
/// column, a spinner and a dot alike.
#[gpui_kit::test]
fn activity_is_said_once_on_the_last_row_in_sight_in_one_column(cx: &mut TestAppContext) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-end-column"], cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    unseen_chat(&shell, cx);
    draw(window, cx);
    let key = cx.read(|cx| shell.read(cx).workspace.branch().unwrap().tabs[0].first().id);
    let (branch, project) = ("sidebar-branch-mark-0-0", "sidebar-project-mark-0");
    for (state, unread, name) in [(Status::Working, false, "spinner"), (Status::Idle, true, "dot")] {
        let tab = format!("sidebar-tab-{key}-{name}");
        let said = |cx: &mut TestAppContext| {
            hover(window, point(px(1000.), px(500.)), cx);
            [tab.as_str(), branch, project].map(|mark| {
                drawn(window, mark.to_owned(), cx).map(|at| at.center().x)
            })
        };
        hear(&shell, ("0001", &roots[0]), (state, None, unread), cx);
        draw(window, cx);
        let [on_tab, none, nor] = said(cx);
        assert!(on_tab.is_some(), "{name}: the tab says it");
        assert_eq!((none, nor), (None, None), "{name}: alone, its tab listed");

        click(window, "sidebar-branch-0-0", cx);
        assert_eq!(said(cx), [None, on_tab, None], "{name}: collapsed, the branch says it there");
        click(window, "sidebar-project-0", cx);
        assert_eq!(said(cx), [None, None, on_tab], "{name}: folded, the project says it there");
        click(window, "sidebar-project-0", cx);
        click(window, "sidebar-branch-0-0", cx);
    }
}

/// A chat at work in no tab has no row of its own, so its branch's row says it while its tabs are
/// listed too.
#[gpui_kit::test]
fn a_chat_at_work_in_no_tab_is_said_on_its_branchs_row(cx: &mut TestAppContext) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-tabless"], cx);
    unseen_chat(&shell, cx);
    draw(window, cx);
    let summary = ThreadSummary {
        status: Status::Working,
        tab_open: false,
        ..crate::chat::tests::summary("0002", &roots[0], "Closed at it", true)
    };
    crate::chat::tests::tell(&shell, summary, cx);
    draw(window, cx);
    assert!(drawn(window, "sidebar-branch-mark-0-0".to_owned(), cx).is_some());
}

/// What an agent runs beside its turn is work too, turn or no turn: how many subagents work is said
/// up and right of the spinner, and the rows a chat's work is said on put theirs together.
#[gpui_kit::test]
fn subagents_and_what_runs_in_the_background_are_said_past_the_turn(cx: &mut TestAppContext) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-beside"], cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let pane = unseen_chat(&shell, cx);
    let runs = |id: &str, subagents, tasks, cx: &mut TestAppContext| {
        let summary = ThreadSummary {
            beside: Beside { subagents, tasks },
            ..crate::chat::tests::summary(id, &roots[0], "A chat", id == "0001")
        };
        crate::chat::tests::tell(&shell, summary, cx);
        draw(window, cx);
    };
    let part = |part: &str, cx: &mut TestAppContext| {
        drawn(window, format!("sidebar-tab-{pane}-{part}"), cx)
    };
    runs("0001", 0, 1, cx);
    let spinner = part("spinner", cx).expect("a shell it left running is work");
    assert_eq!(part("subagents", cx), None);

    runs("0001", 4, 1, cx);
    assert_eq!(part("spinner", cx), Some(spinner), "in its place");
    let count = part("subagents", cx).expect("how many subagents");
    let row = drawn(window, "tab-0-0-0", cx).expect("the chat's row");
    assert!(count.right() > spinner.right() && count.center().y < spinner.center().y);
    assert!(count.right() <= row.right(), "{count:?} in {row:?}");

    runs("0002", 2, 0, cx);
    let (all, waiting) = cx.read(|cx| shell.read(cx).checkout_status(&roots[0], false, cx));
    assert_eq!((all.work.beside.subagents, all.work.beside.tasks, waiting), (6, 1, 0));
    let (unlisted, _) = cx.read(|cx| shell.read(cx).checkout_status(&roots[0], true, cx));
    assert_eq!(unlisted.work.beside.subagents, 2, "the chat in no tab");
}

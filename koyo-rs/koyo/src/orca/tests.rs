//! The Orca sidebar in a test window: its search, a project's row folding its branches away,
//! and projects and branches dragged into the order the person likes.

use std::path::PathBuf;

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext, VisualTestContext, point, px};
use koyo_agent::protocol::model::{Status, ThreadSummary};

use crate::model::PaneKind;
use crate::projects::read_worktrees;
use crate::right_panel::View;
use crate::settings::{self, Layout};
use crate::shell::Shell;
use crate::test_support::{
    center_of, click, draw, drawn, git, hold, hover, let_go, move_held, repo, shell_on, shell_with,
};

/// Whether the sidebar lists the first project's checkout.
fn checkout_listed(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.debug_bounds("orca-branch-0-0").is_some()
}

/// What's typed in the sidebar's search field narrows the worktrees listed to those it names,
/// and Escape, clearing it, lists them all again.
#[gpui_kit::test]
fn typing_in_the_search_field_narrows_the_worktrees_listed(cx: &mut TestAppContext) {
    let (window, _, _) = shell_on("orca-search", Layout::Orca, cx);
    assert!(checkout_listed(window, cx), "the checkout is listed");

    click(window, "sidebar-search", cx);
    cx.simulate_input(window, "nothing");
    assert!(
        !checkout_listed(window, cx),
        "a search that names no worktree lists none"
    );

    cx.simulate_keystrokes(window, "escape");
    assert!(
        checkout_listed(window, cx),
        "the search cleared lists the checkout again"
    );

    cx.simulate_input(window, "mai");
    assert!(
        checkout_listed(window, cx),
        "a search that names the checkout lists it"
    );
}

/// A project's row folds its branches away and brings them back, kept in the settings the Arc
/// sidebar's project name shares; folded, it shows the mark its branch's row showed, a chat at
/// work there.
#[gpui_kit::test]
fn a_projects_row_folds_its_branches_away_and_shows_their_mark(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("orca-fold", Layout::Orca, cx);
    let summary = ThreadSummary {
        status: Status::Working,
        ..crate::chat::tests::summary("0001", &root, "A chat", true)
    };
    let chats = shell.read_with(cx, |shell, _| shell.chats().clone());
    chats.update(cx, |chats, cx| chats.hear_summaries(vec![summary], cx));
    cx.run_until_parked();
    let folded = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).is_folded(&root));
    assert!(drawn(window, "orca-branch-0-0", cx).is_some());
    let mark = |cx: &mut TestAppContext| drawn(window, "orca-project-mark-0", cx).is_some();
    assert!(!mark(cx), "open: the branch's row says it");

    click(window, "orca-project-0", cx);
    assert!(folded(cx));
    assert!(drawn(window, "orca-branch-0-0", cx).is_none(), "its branches folded away");
    assert!(!mark(cx), "under the pointer, New worktree instead");
    hover(window, point(px(900.), px(450.)), cx);
    assert!(mark(cx), "folded: the project's row says it");

    click(window, "sidebar-search", cx);
    cx.simulate_input(window, "mai");
    let found = drawn(window, "orca-project-0", cx).is_some();
    assert!(found, "a search that finds its branch lists it");
    assert!(drawn(window, "orca-branch-0-0", cx).is_none(), "folded still");
    cx.simulate_keystrokes(window, "escape");

    click(window, "orca-new-worktree-0", cx);
    assert!(folded(cx), "its New worktree button leaves it folded");

    click(window, "orca-project-0", cx);
    assert!(!folded(cx));
    assert!(drawn(window, "orca-branch-0-0", cx).is_some(), "its branches back");
    assert!(!mark(cx));
}

/// The Orca sidebar on projects `one` and `two`, `one` with worktrees `feat` and `fix` beside its
/// main checkout: the window, the shell, and the projects' folders.
fn two_projects(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>, [PathBuf; 2]) {
    let roots = ["orca-drag-one", "orca-drag-two"].map(|name| repo(name, &[("README.md", "hi\n")]));
    for branch in ["feat", "fix"] {
        let folder = roots[0].with_file_name(format!("orca-drag-one-{branch}"));
        let _ = std::fs::remove_dir_all(&folder);
        git(&roots[0], &["worktree", "add", "-q", "-b", branch, folder.to_str().unwrap()]);
    }
    let (window, shell) = shell_with(&[&roots[0], &roots[1]], cx);
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Orca));
    shell.update(cx, |shell, cx| {
        shell.show_worktrees(&roots[0], read_worktrees(&roots[0]), cx);
        cx.notify();
    });
    draw(window, cx);
    (window, shell, roots)
}

/// Takes up what `from` names by its middle and lets it go a little past the middle of what `to`
/// names, drawing at each step as a person's drag would be drawn.
fn drag_past(
    window: AnyWindowHandle,
    from: &'static str,
    to: &'static str,
    cx: &mut TestAppContext,
) {
    let from = center_of(window, from, cx);
    let to = center_of(window, to, cx) + point(px(0.), px(4.));
    hold(window, from, cx);
    move_held(window, to, cx);
    draw(window, cx);
    let_go(window, to, cx);
}

/// A project dragged by its row goes to another place among the projects, and a branch dragged
/// by its row among its project's; both orders are saved for the next launch.
#[gpui_kit::test]
fn projects_and_their_branches_are_dragged_into_the_order_the_person_likes(
    cx: &mut TestAppContext,
) {
    let (window, shell, roots) = two_projects(cx);
    let projects = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).projects.clone());
    assert_eq!(projects(cx), roots.to_vec());

    drag_past(window, "orca-project-0", "orca-branch-1-0", cx);
    assert_eq!(projects(cx), [roots[1].clone(), roots[0].clone()], "saved");
    let listed = cx.read(|cx| shell.read(cx).workspace.paths());
    assert_eq!(listed, [roots[1].clone(), roots[0].clone()], "listed");

    let names = |cx: &TestAppContext| {
        cx.read(|cx| {
            let project = &shell.read(cx).workspace.projects[1];
            project.branches.iter().map(|unit| unit.name().to_string()).collect::<Vec<_>>()
        })
    };
    let before = names(cx);
    assert_eq!(before.len(), 3, "{before:?}");
    drag_past(window, "orca-branch-1-0", "orca-branch-1-2", cx);
    let after = names(cx);
    let first_last = [before[1].clone(), before[2].clone(), before[0].clone()];
    assert_eq!(after, first_last, "the first now last");
    let kept = cx.read(|cx| settings::get(cx).worktree_order(&roots[0]).len());
    assert_eq!(kept, 3, "the order saved");
}

/// In the Orca layout the selected worktree's Changes and Files buttons are at the top rail's
/// end, not on its row, and open and close its panel.
#[gpui_kit::test]
fn the_rails_buttons_open_and_close_the_selected_worktrees_panel(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("orca-rail-buttons", Layout::Orca, cx);
    draw(window, cx);
    let button = center_of(window, "toggle-files-0-0", cx);
    assert!(button.y < px(crate::theme::TITLE_BAR_HEIGHT), "in the rail: {button:?}");
    let shown = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).right_panel.shown_view());
    click(window, "toggle-files-0-0", cx);
    assert_eq!(shown(cx), Some(View::Files));
    click(window, "toggle-changes-0-0", cx);
    assert_eq!(shown(cx), Some(View::Changes));
    click(window, "toggle-changes-0-0", cx);
    assert_eq!(shown(cx), None, "pressed again, closed");
}

/// ⌘B folds the Orca layout's sidebar away, compact mode, and leaves its top rail with the
/// selected checkout's tabs, which stay to pick.
#[gpui_kit::test]
fn compact_mode_keeps_the_tabs_in_the_top_rail(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("orca-compact-tabs", Layout::Orca, cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Chat);
        cx.notify();
    });
    draw(window, cx);
    crate::files::tests::press(window, "cmd-b", cx);
    assert!(drawn(window, "orca-branch-0-0", cx).is_none(), "the sidebar folded away");
    let new_tab = center_of(window, "new-tab-button", cx);
    assert!(new_tab.y < px(crate::theme::TITLE_BAR_HEIGHT), "the tabs in the rail: {new_tab:?}");
}

/// A branch's row says how its checkout's doing at its end, as Arc's rows do: the spinner while
/// a chat in it works.
#[gpui_kit::test]
fn a_branch_row_says_its_chat_works_at_its_end(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("orca-branch-mark", Layout::Orca, cx);
    let summary = ThreadSummary {
        status: Status::Working,
        ..crate::chat::tests::summary("0001", &root, "A chat", true)
    };
    let chats = shell.read_with(cx, |shell, _| shell.chats().clone());
    chats.update(cx, |chats, cx| chats.hear_summaries(vec![summary], cx));
    cx.run_until_parked();
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    let row = vcx.debug_bounds("orca-branch-0-0").expect("the branch's row");
    let mark = vcx.debug_bounds("orca-branch-mark-0-0").expect("the spinner on its row");
    assert!(row.right() - mark.right() < px(16.), "{mark:?} at the end of {row:?}");
}

mod rail;

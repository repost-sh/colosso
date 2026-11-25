//! The sidebar's rows in a test window: a project's row folding its branches away, a branch's row
//! one step in from it, and projects and branches dragged into the order the person likes;
//! `space_switch` has the space shown and the switching between spaces, and `tab_places` the tabs
//! moving between the top rail and the sidebar.

use std::path::PathBuf;

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext, VisualTestContext, point, px};
use koyo_agent::protocol::model::{Status, ThreadSummary};

use crate::model::PaneKind;
use crate::projects::read_worktrees;
use crate::right_panel::View;
use crate::settings;
use crate::shell::Shell;
use crate::shell::elements::INDENT;
use crate::test_support::{
    center_of, click, draw, drawn, git, hold, hover, let_go, move_held, repo, shell_on, shell_with,
};

/// A project's row folds its branches away and brings them back, kept in the settings; folded, it
/// shows the mark its branch's row showed, a chat at work there.
#[gpui_kit::test]
fn a_projects_row_folds_its_branches_away_and_shows_their_mark(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("sidebar-fold", cx);
    let summary = ThreadSummary {
        status: Status::Working,
        ..crate::chat::tests::summary("0001", &root, "A chat", true)
    };
    let chats = shell.read_with(cx, |shell, _| shell.chats().clone());
    chats.update(cx, |chats, cx| chats.hear_summaries(vec![summary], cx));
    cx.run_until_parked();
    let folded = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).is_folded(&root));
    assert!(drawn(window, "sidebar-branch-0-0", cx).is_some());
    let mark = |cx: &mut TestAppContext| drawn(window, "sidebar-project-mark-0", cx).is_some();
    assert!(!mark(cx), "open: the branch's row says it");

    click(window, "sidebar-project-0", cx);
    assert!(folded(cx));
    assert!(drawn(window, "sidebar-branch-0-0", cx).is_none(), "its branches folded away");
    assert!(!mark(cx), "under the pointer, New worktree instead");
    hover(window, point(px(900.), px(450.)), cx);
    assert!(mark(cx), "folded: the project's row says it");

    click(window, "sidebar-new-worktree-0", cx);
    assert!(folded(cx), "its New worktree button leaves it folded");

    click(window, "sidebar-project-0", cx);
    assert!(!folded(cx));
    assert!(drawn(window, "sidebar-branch-0-0", cx).is_some(), "its branches back");
    assert!(!mark(cx));
}

/// The sidebar on projects `one` and `two`, `one` with worktrees `feat` and `fix` beside its
/// main checkout: the window, the shell, and the projects' folders.
fn two_projects(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>, [PathBuf; 2]) {
    let roots =
        ["sidebar-drag-one", "sidebar-drag-two"].map(|name| repo(name, &[("README.md", "hi\n")]));
    for branch in ["feat", "fix"] {
        let folder = roots[0].with_file_name(format!("sidebar-drag-one-{branch}"));
        let _ = std::fs::remove_dir_all(&folder);
        git(&roots[0], &["worktree", "add", "-q", "-b", branch, folder.to_str().unwrap()]);
    }
    let (window, shell) = shell_with(&[&roots[0], &roots[1]], cx);
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

    drag_past(window, "sidebar-project-0", "sidebar-branch-1-0", cx);
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
    drag_past(window, "sidebar-branch-1-0", "sidebar-branch-1-2", cx);
    let after = names(cx);
    let first_last = [before[1].clone(), before[2].clone(), before[0].clone()];
    assert_eq!(after, first_last, "the first now last");
    let kept = cx.read(|cx| settings::get(cx).worktree_order(&roots[0]).len());
    assert_eq!(kept, 3, "the order saved");
}

/// With the tabs on the top rail the selected worktree's Changes and Files buttons are at the top
/// rail's end, not on its row, and open and close its panel.
#[gpui_kit::test]
fn the_rails_buttons_open_and_close_the_selected_worktrees_panel(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("rail-buttons", cx);
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

/// ⌘B folds the sidebar away, compact mode, and leaves the top rail with the selected checkout's
/// tabs, which stay to pick.
#[gpui_kit::test]
fn compact_mode_keeps_the_tabs_in_the_top_rail(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("sidebar-compact-tabs", cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Chat);
        cx.notify();
    });
    draw(window, cx);
    crate::test_support::press(window, "cmd-b", cx);
    assert!(drawn(window, "sidebar-branch-0-0", cx).is_none(), "the sidebar folded away");
    let new_tab = center_of(window, "new-tab-button", cx);
    assert!(new_tab.y < px(crate::theme::TITLE_BAR_HEIGHT), "the tabs in the rail: {new_tab:?}");
}

/// A branch's row says how its checkout's doing at its end, as a tab's row does: the spinner while
/// a chat in it works.
#[gpui_kit::test]
fn a_branch_row_says_its_chat_works_at_its_end(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("sidebar-branch-mark", cx);
    let summary = ThreadSummary {
        status: Status::Working,
        ..crate::chat::tests::summary("0001", &root, "A chat", true)
    };
    let chats = shell.read_with(cx, |shell, _| shell.chats().clone());
    chats.update(cx, |chats, cx| chats.hear_summaries(vec![summary], cx));
    cx.run_until_parked();
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    let row = vcx.debug_bounds("sidebar-branch-0-0").expect("the branch's row");
    let mark = vcx.debug_bounds("sidebar-branch-mark-0-0").expect("the spinner on its row");
    assert!(row.right() - mark.right() < px(16.), "{mark:?} at the end of {row:?}");
}

/// A branch's row is as wide as its project's, so its background lit under the pointer or
/// selected is the sidebar's width, and what's in it starts one step further in.
#[gpui_kit::test]
fn a_branchs_row_is_one_step_in_from_its_projects_and_as_wide(cx: &mut TestAppContext) {
    let (window, _, _) = shell_on("sidebar-indent", cx);
    let bounds = |selector: &'static str, cx: &mut TestAppContext| {
        drawn(window, selector, cx).unwrap_or_else(|| panic!("{selector} is drawn"))
    };
    let (project, branch) = (bounds("sidebar-project-0", cx), bounds("sidebar-branch-0-0", cx));
    assert_eq!((branch.left(), branch.size.width), (project.left(), project.size.width));
    let folder = bounds("sidebar-project-icon-0", cx);
    let glyph = bounds("sidebar-branch-icon-0-0", cx);
    assert_eq!(glyph.left() - folder.left(), px(INDENT), "{glyph:?} in from {folder:?}");
}

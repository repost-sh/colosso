//! A branch's row with the tabs in the sidebar (`docs/layout.md`, Sidebar): clicked, it collapses
//! its tabs and opens them again; dragged, it takes the branch to another place among the
//! project's; both are kept for the next launch. Its Changes and Files show its panel. A branch
//! is called by its branch in words.

use std::path::PathBuf;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use koyo_git::{Head, Worktree};

use super::{chat, drag, drawn, sidebar_shell};
use crate::model::BranchUnit;
use crate::projects::read_worktrees;
use crate::right_panel::View;
use crate::settings::{self, BranchPrefix};
use crate::shell::Shell;
use crate::test_support::{center_of, click, draw, git, hover, shell_window};

/// The sidebar with the tabs in it on a project `name` with worktrees `feat` and `fix` beside its
/// main checkout, listed in that order: the window, the shell and the main checkout.
pub(super) fn with_worktrees(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, PathBuf) {
    let (window, shell, roots) = sidebar_shell(&[name], cx);
    let root = roots[0].clone();
    for branch in ["feat", "fix"] {
        let folder = root.with_file_name(format!("{name}-{branch}"));
        let _ = std::fs::remove_dir_all(&folder);
        let folder = folder.to_str().unwrap();
        git(&root, &["worktree", "add", "-q", "-b", branch, folder]);
    }
    shell.update(cx, |shell, cx| {
        shell.show_worktrees(&root, read_worktrees(&root), cx);
        let order = ["main", "feat", "fix"].map(|branch| folder_of(shell, branch));
        settings::update(cx, |settings| {
            settings.set_worktree_order(&root, order.to_vec());
        });
        shell.show_worktrees(&root, read_worktrees(&root), cx);
        cx.notify();
    });
    draw(window, cx);
    assert_eq!(listed(&shell, cx), ["main", "feat", "fix"]);
    (window, shell, root)
}

/// The folder of the worktree on `branch` of the first project.
pub(super) fn folder_of(shell: &Shell, branch: &str) -> PathBuf {
    let units = &shell.workspace.projects[0].branches;
    let unit = (units.iter()).find(|unit| unit.name() == branch);
    unit.expect("the worktree").worktree.path.clone()
}

/// The first project's worktrees, by name, in the order they're listed.
fn listed(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let units = &shell.read(cx).workspace.projects[0].branches;
        units.iter().map(|unit| unit.name().to_string()).collect()
    })
}

/// Closes the window of `shell`, as quitting does, and opens the app's window again, as the
/// next launch does: what it shows then.
fn relaunch(
    window: AnyWindowHandle,
    shell: Entity<Shell>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>) {
    drop(shell);
    (cx.update_window(window, |_, window, _| window.remove_window())).unwrap();
    cx.run_until_parked();
    let (window, shell) = shell_window(cx);
    draw(window, cx);
    (window, shell)
}

#[gpui_kit::test]
fn a_branchs_row_collapses_its_tabs_and_opens_them_again_and_the_next_launch_keeps_it(
    cx: &mut TestAppContext,
) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-collapse"], cx);
    let root = roots[0].clone();
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("one"));
        cx.notify();
    });
    draw(window, cx);
    let shows = |window, cx: &mut TestAppContext| {
        assert!(drawn(window, "sidebar-branch-0-0", cx).is_some(), "the branch's row");
        drawn(window, "tab-0-0-0", cx).is_some()
    };
    let collapsed =
        |cx: &TestAppContext| cx.read(|cx| settings::get(cx).is_collapsed(&root, &root));
    assert!(shows(window, cx));
    assert!(!collapsed(cx));

    click(window, "sidebar-branch-0-0", cx);
    assert!(collapsed(cx));
    assert!(
        !shows(window, cx),
        "collapsed: its tabs hidden, the tab shown too"
    );
    let tab_shown = cx.read(|cx| shell.read(cx).workspace.shown_tab());
    assert_eq!(tab_shown, Some((0, 0, 0)), "still the tab shown");

    let (window, shell) = relaunch(window, shell, cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("one"));
        cx.notify();
    });
    draw(window, cx);
    assert!(collapsed(cx));
    assert!(!shows(window, cx), "collapsed as it was left");
    click(window, "sidebar-branch-0-0", cx);
    assert!(!collapsed(cx));
    assert!(shows(window, cx), "open again");
    let kept = cx.read(|cx| settings::get(cx).project_looks.contains_key(&root));
    assert!(!kept);
}

/// Presses `button` on the branch's row `row` names, the pointer brought onto the row first, as
/// a person's is: its buttons show only then.
fn press_on(
    window: AnyWindowHandle,
    (row, button): (&'static str, &'static str),
    cx: &mut TestAppContext,
) {
    hover(window, center_of(window, row, cx), cx);
    draw(window, cx);
    click(window, button, cx);
}

/// With Changes and Files on the worktree, a branch's row's changes' and files' buttons show the
/// worktree's panel on them, selecting the worktree first if it isn't, and close it pressed
/// again; the row stays as it was.
#[gpui_kit::test]
fn a_branchs_row_buttons_show_its_worktrees_panel_and_close_it(cx: &mut TestAppContext) {
    let (window, shell, root) = with_worktrees("sidebar-panel-buttons", cx);
    let worktree = settings::ChangesAndFiles::Worktree;
    cx.update(|cx| settings::update(cx, |settings| settings.changes_and_files = worktree));
    let feat = cx.read(|cx| folder_of(shell.read(cx), "feat"));
    let shown = |cx: &TestAppContext| {
        cx.read(|cx| {
            let shell = shell.read(cx);
            let selected = shell.workspace.branch().map(|unit| unit.worktree.path.clone());
            (selected, shell.right_panel.shown_view())
        })
    };
    let main = Some(root.clone());
    assert_eq!(shown(cx), (main.clone(), None));
    assert!(drawn(window, "toggle-files-0-1", cx).is_none(), "away from the row, none");

    press_on(window, ("sidebar-branch-0-1", "toggle-files-0-1"), cx);
    assert_eq!(shown(cx), (Some(feat.clone()), Some(View::Files)), "feat selected, its files");
    let collapsed = cx.read(|cx| settings::get(cx).is_collapsed(&root, &feat));
    assert!(!collapsed, "the press is the button's, not the row's");

    hover(window, center_of(window, "sidebar-branch-0-0", cx), cx);
    draw(window, cx);
    assert!(drawn(window, "toggle-files-0-1", cx).is_some(), "lit, it stays");
    assert!(drawn(window, "toggle-changes-0-1", cx).is_none());
    press_on(window, ("sidebar-branch-0-1", "toggle-changes-0-1"), cx);
    assert_eq!(shown(cx), (Some(feat.clone()), Some(View::Changes)));
    press_on(window, ("sidebar-branch-0-1", "toggle-changes-0-1"), cx);
    assert_eq!(shown(cx), (Some(feat), None), "pressed again, closed");

    press_on(window, ("sidebar-branch-0-0", "toggle-changes-0-0"), cx);
    assert_eq!(shown(cx), (main, Some(View::Changes)), "main selected, its changes");
}

#[gpui_kit::test]
fn a_worktree_dragged_by_its_row_moves_and_the_next_launch_keeps_it(
    cx: &mut TestAppContext,
) {
    let (window, shell, root) = with_worktrees("sidebar-reorder", cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.select_branch(0, 2);
        shell.workspace.push_tab(chat("one"));
        shell.workspace.select_branch(0, 0);
        cx.notify();
    });
    draw(window, cx);
    drag(window, "sidebar-branch-0-0", "tab-0-2-0", cx);
    assert_eq!(listed(&shell, cx), ["feat", "fix", "main"]);
    let selected = cx.read(|cx| shell.read(cx).workspace.branch().map(BranchUnit::name));
    assert_eq!(selected.as_deref(), Some("main"), "still selected");
    let kept = cx.read(|cx| settings::get(cx).worktree_order(&root).to_vec());
    let order = shell.read_with(cx, |shell, _| shell.workspace.projects[0].worktree_paths());
    assert_eq!(kept, order, "kept as listed");

    let (window, shell) = relaunch(window, shell, cx);
    assert_eq!(
        listed(&shell, cx),
        ["feat", "fix", "main"],
        "as it was left"
    );
    drag(window, "sidebar-branch-0-2", "sidebar-branch-0-0", cx);
    assert_eq!(listed(&shell, cx), ["main", "feat", "fix"]);
}

/// A worktree of `tabs` tabs at `path`.
fn unit(path: &str, is_main: bool, tabs: usize) -> BranchUnit {
    BranchUnit {
        worktree: Worktree {
            path: path.into(),
            head: Head::Branch("feat".into()),
            is_main,
        },
        tabs: (0..tabs).map(|_| chat("a chat")).collect(),
        active_tab: 0,
    }
}

#[test]
fn a_worktree_is_called_by_its_branch_in_words_without_the_persons_prefix() {
    let named = |branch: &str| {
        let mut named = unit("/work/x", false, 0);
        named.worktree.head = Head::Branch(branch.into());
        named
    };
    let called = |branch, prefix| named(branch).title(prefix).to_string();
    let mine = Some("schulzfel");
    assert_eq!(called("schulzfel/some-nasty-name", mine), "Some nasty name");
    assert_eq!(called("schulzfel/fix_login", mine), "Fix login");
    assert_eq!(called("main", mine), "Main");
    assert_eq!(
        called("feat/login", mine),
        "Feat/login",
        "another prefix stays"
    );
    assert_eq!(
        called("schulzfelix/x", mine),
        "Schulzfelix/x",
        "only the whole prefix"
    );
    assert_eq!(
        called("schulzfel/x", None),
        "Schulzfel/x",
        "no prefix known"
    );
    let mut detached = named("x");
    detached.worktree.head = Head::Detached("0123456789abcdef".into());
    assert_eq!(detached.title(mine), "detached at 0123456");
}

#[test]
fn the_persons_own_prefix_is_the_one_new_worktrees_get() {
    let user = Some("schulzfel");
    assert_eq!(BranchPrefix::GitHubUser.own(user), user);
    assert_eq!(BranchPrefix::GitHubUser.own(None), None, "no user known");
    let custom = BranchPrefix::Custom(" team ".into());
    assert_eq!(custom.own(user), Some("team"));
    assert_eq!(
        BranchPrefix::Custom("  ".into()).own(user),
        None,
        "an empty one is none"
    );
    assert_eq!(BranchPrefix::None.own(user), None);
}

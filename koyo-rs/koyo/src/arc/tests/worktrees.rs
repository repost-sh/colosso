//! A worktree's header in the Arc sidebar (`docs/arc-sidebar.md` 5): clicked, it collapses its
//! worktree and opens it again; dragged, it takes the worktree to another place among the
//! project's, in both layouts' lists; both are kept for the next launch. Its menu offers what the
//! worktree can do, its `+` opens a new tab on it, and a tab from another worktree let go on it
//! moves there, collapsed or not.

use std::path::PathBuf;

use gpui_kit::{Action, AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use koyo_git::{Head, Worktree};

use super::{arc_shell, chat, drag, drawn};
use crate::arc::actions::ClearWorktree;
use crate::arc::header::{Items, title};
use crate::files::tests::{dialog, press};
use crate::launcher::Launcher;
use crate::model::BranchUnit;
use crate::projects::{DeleteWorktree, read_worktrees};
use crate::settings::{self, BranchPrefix, Layout};
use crate::shell::Shell;
use crate::test_support::{click, draw, git, shell_window};

/// The Arc sidebar on a project `name` with worktrees `feat` and `fix` beside its main
/// checkout, listed in that order: the window, the shell and the main checkout.
pub(super) fn with_worktrees(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, PathBuf) {
    let (window, shell, roots) = arc_shell(&[name], cx);
    let root = roots[0].clone();
    for branch in ["feat", "fix"] {
        let folder = root.with_file_name(format!("{name}-{branch}"));
        let _ = std::fs::remove_dir_all(&folder);
        let folder = folder.to_str().unwrap();
        git(&root, &["worktree", "add", "-q", "-b", branch, folder]);
    }
    shell.update(cx, |shell, cx| {
        shell.show_worktrees(&root, read_worktrees(&root), cx);
        // Main, feat, fix, whatever order git lists them in: kept, as a drag keeps it, and
        // followed as git lists them again.
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
fn a_header_collapses_its_worktree_and_opens_it_again_and_the_next_launch_keeps_it(
    cx: &mut TestAppContext,
) {
    let (window, shell, roots) = arc_shell(&["arc-collapse"], cx);
    let root = roots[0].clone();
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("one"));
        cx.notify();
    });
    draw(window, cx);
    // Whether its tab is drawn; its header always is.
    let shows = |window, cx: &mut TestAppContext| {
        assert!(drawn(window, "arc-header-0", cx).is_some(), "the header");
        drawn(window, "tab-0-0-0", cx).is_some()
    };
    let collapsed =
        |cx: &TestAppContext| cx.read(|cx| settings::get(cx).is_collapsed(&root, &root));
    assert!(shows(window, cx));
    assert!(!collapsed(cx));

    click(window, "arc-header-0", cx);
    assert!(collapsed(cx));
    assert!(
        !shows(window, cx),
        "collapsed: its tabs hidden, the tab shown too"
    );
    let tab_shown = cx.read(|cx| shell.read(cx).workspace.shown_tab());
    assert_eq!(tab_shown, Some((0, 0, 0)), "still the tab shown");

    let (window, shell) = relaunch(window, shell, cx);
    // A test's window keeps no tabs, so the one coming back is given one again.
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("one"));
        cx.notify();
    });
    draw(window, cx);
    assert!(collapsed(cx));
    assert!(!shows(window, cx), "collapsed as it was left");
    click(window, "arc-header-0", cx);
    assert!(!collapsed(cx));
    assert!(shows(window, cx), "open again");
    // Nothing kept of a worktree that's open.
    let kept = cx.read(|cx| settings::get(cx).project_looks.contains_key(&root));
    assert!(!kept);
}

#[gpui_kit::test]
fn a_worktree_dragged_by_its_header_moves_in_both_layouts_and_the_next_launch_keeps_it(
    cx: &mut TestAppContext,
) {
    let (window, shell, root) = with_worktrees("arc-reorder", cx);
    // A tab on the last worktree, which the main checkout is let go on, past that worktree's
    // middle, taken by its header.
    shell.update(cx, |shell, cx| {
        shell.workspace.select_branch(0, 2);
        shell.workspace.push_tab(chat("one"));
        shell.workspace.select_branch(0, 0);
        cx.notify();
    });
    draw(window, cx);
    drag(window, "arc-header-0", "tab-0-2-0", cx);
    assert_eq!(listed(&shell, cx), ["feat", "fix", "main"]);
    let selected = cx.read(|cx| shell.read(cx).workspace.branch().map(BranchUnit::name));
    assert_eq!(selected.as_deref(), Some("main"), "still selected");
    let kept = cx.read(|cx| settings::get(cx).worktree_order(&root).to_vec());
    let order = shell.read_with(cx, |shell, _| shell.workspace.projects[0].worktree_paths());
    assert_eq!(kept, order, "kept as listed");

    // Orca lists the same worktrees: the order is the project's, not the sidebar's.
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Orca));
    draw(window, cx);
    assert_eq!(listed(&shell, cx), ["feat", "fix", "main"]);
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Arc));

    let (window, shell) = relaunch(window, shell, cx);
    assert_eq!(
        listed(&shell, cx),
        ["feat", "fix", "main"],
        "as it was left"
    );
    // Back up, over the first worktree's header.
    drag(window, "arc-header-2", "arc-header-0", cx);
    assert_eq!(listed(&shell, cx), ["main", "feat", "fix"]);
}

#[gpui_kit::test]
fn a_tab_let_go_on_another_worktrees_header_moves_there_open_or_collapsed(cx: &mut TestAppContext) {
    let (window, shell, _) = with_worktrees("arc-drop-on-header", cx);
    shell.update(cx, |shell, cx| {
        for title in ["one", "two", "three"] {
            shell.workspace.push_tab(chat(title));
        }
        cx.notify();
    });
    draw(window, cx);
    let titles = |branch: usize, cx: &TestAppContext| -> Vec<String> {
        cx.read(|cx| {
            let units = &shell.read(cx).workspace.projects[0].branches;
            (units[branch].tabs.iter())
                .map(|tab| tab.first().title.to_string())
                .collect()
        })
    };
    let shown = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.shown_tab());

    drag(window, "tab-0-0-0", "arc-header-1", cx);
    assert_eq!(titles(0, cx), ["two", "three"], "it left its own worktree");
    assert_eq!(titles(1, cx), ["one"], "and went to the other's");
    assert_eq!(shown(cx), Some((0, 1, 0)), "where it shows");

    // On a collapsed header, it goes first among the worktree's tabs, which stay hidden.
    click(window, "arc-header-2", cx);
    drag(window, "tab-0-0-0", "arc-header-2", cx);
    assert_eq!(titles(0, cx), ["three"]);
    assert_eq!(titles(2, cx), ["two"]);
    assert_eq!(shown(cx), Some((0, 2, 0)));
    assert!(drawn(window, "tab-0-2-0", cx).is_none(), "still collapsed");
}

#[gpui_kit::test]
fn a_headers_plus_opens_a_new_tab_on_its_worktree_opening_it_if_collapsed(cx: &mut TestAppContext) {
    let (window, shell, root) = with_worktrees("arc-plus", cx);
    let fix = shell.read_with(cx, |shell, _| folder_of(shell, "fix"));
    // The launcher it opens, if it opened one, dismissed again; and the worktree selected.
    let plus = |branch: &'static str, cx: &mut TestAppContext| {
        click(window, branch, cx);
        let launcher = dialog::<Launcher>(cx).map(|(launcher, _)| launcher);
        if let Some(launcher) = launcher {
            press(launcher, "escape", cx);
            cx.run_until_parked();
        }
        let selected = cx.read(|cx| shell.read(cx).workspace.branch().map(BranchUnit::name));
        (launcher.is_some(), selected.map(String::from))
    };
    let opened = plus("arc-header-new-tab-1", cx);
    assert_eq!(opened, (true, Some("feat".into())), "the launcher, on feat");

    click(window, "arc-header-2", cx);
    let collapsed = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).is_collapsed(&root, &fix));
    assert!(collapsed(cx));
    let opened = plus("arc-header-new-tab-2", cx);
    assert_eq!(opened, (true, Some("fix".into())));
    assert!(!collapsed(cx), "opened, so the tab it opens shows");
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

/// What the menu says of the worktree's own items, item by item: what each is called and
/// whether it can be picked, with what each does checked.
fn offers(unit: &BranchUnit) -> Vec<(&'static str, bool)> {
    let path = unit.worktree.path.clone();
    let does: [Box<dyn Action>; 2] = [
        Box::new(ClearWorktree(path.clone())),
        Box::new(DeleteWorktree(path)),
    ];
    let items = Items::of(unit).own();
    for ((label, _, action), expected) in items.iter().zip(&does) {
        assert!(action.partial_eq(expected.as_ref()), "{label}");
    }
    items
        .into_iter()
        .map(|(label, can, _)| (label, can))
        .collect()
}

#[test]
fn the_menu_offers_clearing_and_deleting_as_the_worktree_can_be() {
    let (clear, delete) = (("Clear Tabs…", true), ("Delete Worktree…", true));
    assert_eq!(offers(&unit("/work/feat", false, 2)), [clear, delete]);
    // No tabs, nothing to clear.
    let nothing = ("Clear Tabs…", false);
    assert_eq!(offers(&unit("/work/feat", false, 0)), [nothing, delete]);
    // Git can't delete a main checkout.
    let main = ("Delete Worktree…", false);
    assert_eq!(offers(&unit("/src/app", true, 1)), [clear, main]);
    assert_eq!(offers(&unit("/src/app", true, 0)), [nothing, main]);
}

#[test]
fn a_worktree_is_called_by_its_branch_in_words_without_the_persons_prefix() {
    let named = |branch: &str| {
        let mut named = unit("/work/x", false, 0);
        named.worktree.head = Head::Branch(branch.into());
        named
    };
    let called = |branch, prefix| title(&named(branch), prefix).to_string();
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
    assert_eq!(title(&detached, mine), "detached at 0123456");
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

use std::path::{Path, PathBuf};

use gpui_kit::{
    AnyWindowHandle, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowOptions, px, size,
};

use super::*;
use crate::projects::read_worktrees;
use crate::test_support::{git, scratch};

/// A repository of its own, with a commit, for one checkout in the sidebar.
fn repository(name: &str) -> PathBuf {
    let root = scratch(name);
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "first"]);
    root
}

/// The shell in a window of its own, with each of `roots` a project in its sidebar.
fn shell_with(roots: &[&Path], cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    // Each checkout's project lists and watches it on a thread of its own.
    cx.executor().allow_parking();
    cx.update(crate::init);
    let (window, shell) = cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1400.), px(900.)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..WindowOptions::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Shell::new(window, cx))
        })
        .unwrap()
    });
    shell.update(cx, |shell, cx| {
        for root in roots {
            shell.workspace.add_project(root.to_path_buf());
            shell.show_worktrees(root, read_worktrees(root), cx);
        }
    });
    (window, shell)
}

fn select(shell: &Entity<Shell>, root: &Path, cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| {
        let project = shell.workspace.project_index(root).unwrap();
        assert!(shell.workspace.select_worktree(project, root));
        cx.notify();
    });
    cx.run_until_parked();
}

/// The selected checkout and its file tree, as the right panel holds them.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> (Entity<Checkout>, Entity<FilesPanel>) {
    cx.read(|cx| {
        let panels = shell.read(cx).right_panel.shown().unwrap();
        (panels.checkout.clone(), panels.files.clone().unwrap())
    })
}

#[gpui_kit::test]
fn each_checkout_keeps_its_panels_until_it_leaves_the_sidebar(cx: &mut TestAppContext) {
    let (one, two) = (repository("right-panel-one"), repository("right-panel-two"));
    let (window, shell) = shell_with(&[&one, &two], cx);
    select(&shell, &one, cx);
    cx.dispatch_action(window, ShowFiles);
    cx.run_until_parked();
    let first = shown(&shell, cx);
    select(&shell, &two, cx);
    let second = shown(&shell, cx);
    assert_ne!(first.1, second.1, "each checkout has a tree of its own");
    // Back to the first: the same checkout and tree, as they were left.
    select(&shell, &one, cx);
    assert_eq!(shown(&shell, cx), first);
    // The second leaves the sidebar, and its panels with it.
    shell.update(cx, |shell, cx| {
        let project = shell.workspace.project_index(&two).unwrap();
        shell.workspace.remove_project(project);
        cx.notify();
    });
    cx.run_until_parked();
    let kept: Vec<PathBuf> = cx.read(|cx| {
        shell
            .read(cx)
            .right_panel
            .checkouts
            .keys()
            .cloned()
            .collect()
    });
    assert_eq!(kept, std::slice::from_ref(&one));
    for root in [one, two] {
        let _ = std::fs::remove_dir_all(root);
    }
}

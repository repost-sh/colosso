use std::path::PathBuf;

use gpui_kit::{
    Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, point,
};

use super::*;
use crate::test_support::{repo, select, shell_with};

/// The selected checkout and its file tree, as the right panel holds them.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> (Entity<Checkout>, Entity<FilesPanel>) {
    cx.read(|cx| {
        let panels = shell.read(cx).right_panel.shown().unwrap();
        (panels.checkout.clone(), panels.files.clone().unwrap())
    })
}

#[gpui_kit::test]
fn each_checkout_keeps_its_panels_until_it_leaves_the_sidebar(cx: &mut TestAppContext) {
    let (one, two) = (repo("right-panel-one", &[]), repo("right-panel-two", &[]));
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

/// Drags the handle `selector` names `by` across, in steps, and lets go only once `between`
/// has looked at the drag under way. The edge goes where the pointer is, so it moves as far
/// as the pointer give or take where on the handle it was grabbed.
fn drag(
    selector: &'static str,
    by: f32,
    cx: &mut VisualTestContext,
    between: impl FnOnce(&mut VisualTestContext),
) {
    let from: Point<Pixels> = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    let to = |step: f32| point(from.x + px(by * step / 10.), from.y);
    for step in 1..=10 {
        cx.simulate_mouse_move(to(step as f32), MouseButton::Left, Modifiers::none());
    }
    between(cx);
    cx.simulate_mouse_up(to(10.), MouseButton::Left, Modifiers::none());
}

/// How far from an edge its handle is grabbed at its middle: half the handle's 6 points, the
/// border inside it, and a point to spare.
const GRAB: f32 = 5.;

#[gpui_kit::test]
#[expect(
    clippy::float_cmp,
    reason = "a width not saved is the one read before, not near it"
)]
fn a_dragged_edge_moves_as_it_goes_and_is_saved_once_let_go(cx: &mut TestAppContext) {
    let root = repo("right-panel-drag", &[("a.txt", "a\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    cx.dispatch_action(window, ShowFiles);
    cx.run_until_parked();
    let cx = &mut VisualTestContext::from_window(window, cx);
    let saved = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            let settings = settings::get(cx);
            (settings.files_tree_width, settings.files_panel_width)
        })
    };
    let (tree, panel) = saved(cx);
    let edge = |selector, cx: &mut VisualTestContext| {
        cx.debug_bounds(selector).unwrap().center().x.as_f32()
    };
    // The tree's edge, beside the file.
    let before = edge("files-tree-resize", cx);
    drag("files-tree-resize", 60., cx, |cx| {
        let after = edge("files-tree-resize", cx);
        assert!((after - before - 60.).abs() < GRAB, "{before} to {after}");
        assert_eq!(saved(cx), (tree, panel), "nothing's saved mid-drag");
    });
    assert!((saved(cx).0 - tree - 60.).abs() < GRAB, "{:?}", saved(cx));
    // The panel's own edge, beside the chat, as wide as the window lets it be: dragged
    // right, it's narrower.
    let wide = cx.read(|cx| shell.read(cx).right_panel.width(cx));
    let before = edge("right-panel-resize", cx);
    drag("right-panel-resize", 40., cx, |cx| {
        let after = edge("right-panel-resize", cx);
        assert!((after - before - 40.).abs() < GRAB, "{before} to {after}");
        assert_eq!(saved(cx).1, panel, "nothing's saved mid-drag");
    });
    assert!((saved(cx).1 - (wide - 40.)).abs() < GRAB, "{:?}", saved(cx));
    let _ = std::fs::remove_dir_all(root);
}

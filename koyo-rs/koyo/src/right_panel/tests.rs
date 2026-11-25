use std::path::PathBuf;

use gpui_kit::{
    Action, AnyWindowHandle, CursorStyle, Entity, Modifiers, MouseButton, Pixels, Point,
    TestAppContext, VisualTestContext, point,
};

use super::*;
use crate::files::tests::shell_showing;
use crate::settings::{ChangesAndFiles, SetChangesAndFiles, Tabs};
use crate::shell::ToggleRightPanel;
use crate::test_support::{
    GRAB, act, center_of, click, cursor_at, drag, draw, drawn, hover, open_page, pin, repo, select,
    shell_on, shell_on_chat, shell_with,
};

/// The width the Files panel is set to.
fn files_width(cx: &TestAppContext) -> f32 {
    cx.read(|cx| settings::get(cx).files_tree_width)
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
    let (one, two) = (repo("right-panel-one", &[]), repo("right-panel-two", &[]));
    let (window, shell) = shell_with(&[&one, &two], cx);
    select(&shell, &one, cx);
    cx.dispatch_action(window, ShowFiles);
    cx.run_until_parked();
    let first = shown(&shell, cx);
    select(&shell, &two, cx);
    cx.dispatch_action(window, ShowFiles);
    cx.run_until_parked();
    let second = shown(&shell, cx);
    assert_ne!(first.1, second.1, "each checkout has a tree of its own");
    select(&shell, &one, cx);
    assert_eq!(shown(&shell, cx), first);
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

/// Each checkout keeps whether its panel's open and on which: going to another finds that one's
/// as it was left, closed until it's opened there, and coming back finds the first's.
#[gpui_kit::test]
fn each_checkout_keeps_whether_its_panel_is_open_and_on_which(cx: &mut TestAppContext) {
    let (one, two) = (repo("right-panel-side-one", &[]), repo("right-panel-side-two", &[]));
    let (window, shell) = shell_with(&[&one, &two], cx);
    let shown = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).right_panel.shown_view());
    select(&shell, &one, cx);
    cx.dispatch_action(window, ShowFiles);
    cx.run_until_parked();
    assert_eq!(shown(cx), Some(View::Files));

    select(&shell, &two, cx);
    assert_eq!(shown(cx), None, "the other checkout's panel is closed");
    cx.dispatch_action(window, ToggleChanges);
    cx.run_until_parked();
    assert_eq!(shown(cx), Some(View::Changes));

    select(&shell, &one, cx);
    assert_eq!(shown(cx), Some(View::Files), "the first's as it was left");
    cx.dispatch_action(window, ToggleFiles);
    cx.run_until_parked();
    select(&shell, &two, cx);
    assert_eq!(shown(cx), Some(View::Changes), "closing one leaves the other's");
    for root in [one, two] {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// Drags what's at `from` `by` across, in steps, and lets go only once `between` has looked at
/// the drag under way. The edge goes where the pointer is, so it moves as far as the pointer
/// give or take where on the handle it was grabbed.
fn drag_and_look(
    from: Point<Pixels>,
    by: f32,
    cx: &mut VisualTestContext,
    between: impl FnOnce(&mut VisualTestContext),
) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    let to = |step: f32| point(from.x + px(by * step / 10.), from.y);
    for step in 1..=10 {
        cx.simulate_mouse_move(to(step as f32), MouseButton::Left, Modifiers::none());
    }
    between(cx);
    cx.simulate_mouse_up(to(10.), MouseButton::Left, Modifiers::none());
}

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
    let set = files_width(cx);
    let edge = |selector, cx: &mut VisualTestContext| {
        cx.debug_bounds(selector).unwrap().center().x.as_f32()
    };
    let before = edge("right-panel-resize", cx);
    let from = cx.debug_bounds("right-panel-resize").unwrap().center();
    drag_and_look(from, -60., cx, |cx| {
        let after = edge("right-panel-resize", cx);
        assert!((before - after - 60.).abs() < GRAB, "{before} to {after}");
        assert_eq!(files_width(cx), set, "nothing's saved mid-drag");
    });
    let saved = files_width(cx);
    assert!((saved - set - 60.).abs() < GRAB, "{set} to {saved}");
    let _ = std::fs::remove_dir_all(root);
}

/// The edge, the glass between the file shown's island and the panel's, is picked up across all
/// of it, the resize cursor over all of it.
#[gpui_kit::test]
fn the_edge_is_picked_up_across_the_glass_between_the_islands(cx: &mut TestAppContext) {
    let root = repo("right-panel-reach", &[("README.md", "hi\n")]);
    let (window, _, _) = shell_showing(&root, cx);
    let near = theme::GUTTER / 2. - 1.;
    draw(window, cx);
    for reach in [-near, near] {
        picked_up(window, reach, &format!("{reach} from the edge"), cx);
    }
    let _ = std::fs::remove_dir_all(root);
}

/// Picks the edge up `reach` across from its middle and drags it 30 points left: the resize
/// cursor shows there, and the panel widens by about as far.
fn picked_up(window: AnyWindowHandle, reach: f32, what: &str, cx: &mut TestAppContext) {
    let middle = center_of(window, "right-panel-resize", cx);
    let at = point(middle.x + px(reach), middle.y);
    assert_eq!(
        cursor_at(window, at, cx),
        CursorStyle::ResizeColumn,
        "{what}"
    );
    let set = files_width(cx);
    drag(window, at, at - point(px(30.), px(0.)), cx);
    let grown = files_width(cx) - set;
    assert!((grown - 30.).abs() < 10., "{what}: it widened {grown}");
}

/// What the Toggle Changes and Toggle Files actions do: each opens the panel on its view and
/// closes it, and the panel opens again after it's closed, though what had the keyboard went
/// with it.
#[gpui_kit::test]
fn a_panel_closed_by_its_action_opens_again(cx: &mut TestAppContext) {
    let root = repo("right-panel-reopen", &[("a.txt", "a\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    toggles(&shell, window, View::Changes, &ToggleChanges, cx);
    toggles(&shell, window, View::Files, &ToggleFiles, cx);
    let _ = std::fs::remove_dir_all(root);
}

/// Toggles `view` open and closed twice with `toggle`, as its button does.
fn toggles(
    shell: &Entity<Shell>,
    window: AnyWindowHandle,
    view: View,
    toggle: &(impl Action + Clone),
    cx: &mut TestAppContext,
) {
    for open in [true, false, true, false] {
        cx.dispatch_action(window, toggle.clone());
        cx.run_until_parked();
        let shows = cx.read(|cx| shell.read(cx).right_panel.shows(view));
        assert_eq!(shows, open, "{view:?} after toggling it to {open}");
    }
}

/// Changes and Files placed on the worktree from the buttons' menu, the branch's row has them
/// while the pointer's on it; placed on the top rail, the row has none, the rail having them.
/// With the tabs in the sidebar there's no top rail, so they're on the worktree whatever's
/// stored, and what's stored is kept for the tabs going back to the rail.
#[gpui_kit::test]
fn the_changes_and_files_buttons_go_where_their_menu_places_them(cx: &mut TestAppContext) {
    let (window, _, root) = shell_on("right-panel-placement", cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let on_row = |cx: &mut TestAppContext| {
        hover(window, center_of(window, "sidebar-branch-0-0", cx), cx);
        let row = drawn(window, "sidebar-branch-0-0", cx).expect("the branch's row");
        drawn(window, "toggle-files-0-0", cx).is_some_and(|button| row.contains(&button.center()))
    };
    let on_rail = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).panel_buttons_on_rail());
    let tabs_in = |tabs: Tabs, cx: &mut TestAppContext| {
        cx.update(|cx| settings::update(cx, |settings| settings.tabs = tabs));
    };

    cx.dispatch_action(window, SetChangesAndFiles(ChangesAndFiles::Worktree));
    assert!(on_row(cx), "on the worktree");
    assert!(!on_rail(cx));
    cx.dispatch_action(window, SetChangesAndFiles(ChangesAndFiles::TopRail));
    assert!(!on_row(cx), "on the rail, not the worktree");
    assert!(on_rail(cx));

    tabs_in(Tabs::Sidebar, cx);
    assert!(on_row(cx), "no rail with the tabs in the sidebar: on the worktree");
    assert!(!on_rail(cx));
    let kept = cx.read(|cx| settings::get(cx).changes_and_files);
    assert_eq!(kept, ChangesAndFiles::TopRail, "the choice kept");
    tabs_in(Tabs::TopRail, cx);
    assert!(on_rail(cx), "back on the rail with the tabs");
    let _ = std::fs::remove_dir_all(root);
}

/// A branch's panel is beside its own tabs alone: a pinned or an unpinned page shown, no branch's,
/// hides it, with its button's light, and the branch's tab shown again finds it as it was. Asked
/// for over such a page, it shows beside its branch's tab.
#[gpui_kit::test]
fn a_branchs_panel_hides_while_a_page_of_no_branch_shows(cx: &mut TestAppContext) {
    let (window, shell) = shell_on_chat("right-panel-apart", cx);
    let key = pin(window, &shell, &["https://a.example/"], cx)[0];
    let panel = |cx: &mut TestAppContext| drawn(window, "right-panel-resize", cx).is_some();
    let on_page = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.apart_shown());
    let show_page = |cx: &mut TestAppContext| {
        shell.update(cx, |shell, cx| {
            let at = shell.workspace.pinned_at(key).expect("pinned");
            assert!(shell.workspace.show_pinned(at));
            cx.notify();
        });
        draw(window, cx);
    };
    click(window, "tab-0-0-0", cx);
    act(window, ToggleChanges, cx);
    assert!(panel(cx), "beside the branch's tab");
    show_page(cx);
    assert!(!panel(cx), "a pinned page shows");
    click(window, "tab-0-0-0", cx);
    assert!(panel(cx), "back with the branch's tab");

    let toggles: [&dyn Action; 2] = [&ToggleChanges, &ToggleRightPanel];
    for toggle in toggles {
        show_page(cx);
        cx.update_window(window, |_, window, cx| window.dispatch_action(toggle.boxed_clone(), cx))
            .unwrap();
        cx.run_until_parked();
        draw(window, cx);
        assert!(panel(cx) && !on_page(cx), "{}: asked for, beside its branch's tab", toggle.name());
    }
    act(window, ToggleRightPanel, cx);
    assert!(!panel(cx), "closed while it's there");

    act(window, ToggleChanges, cx);
    open_page(window, &shell, Some("https://b.example/"), cx);
    shell.update(cx, |shell, cx| {
        let at = shell.workspace.shown_tab().expect("the page's tab");
        assert!(shell.workspace.loosen_tab(at, usize::MAX));
        cx.notify();
    });
    draw(window, cx);
    assert!(on_page(cx) && !panel(cx), "an unpinned page shows");
}

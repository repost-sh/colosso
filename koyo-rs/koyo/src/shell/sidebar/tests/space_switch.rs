//! The spaces in the sidebar (`docs/layout.md`): the sidebar lists the projects of the space
//! shown, a space with none the row that adds one; a mark at its foot shows its space at the
//! project it showed last; ⌥⇧← and ⌥⇧→, and ⌘⌥⇧← and ⌘⌥⇧→, show the space beside, and ⌥← and ⌥→
//! the project row beside, on into the space beside past the first or last and over a space with
//! none; ⌥↑ and ⌥↓ the tab beside in the order the tabs are listed; at either end each gives a
//! little and springs back. The top rail's pieces of the space move with it, and with reduced
//! motion two fingers let go switch at once.

use std::path::PathBuf;

use gpui_kit::{AnyWindowHandle, AppContext as _, Bounds, Entity, Pixels, TestAppContext, TouchPhase};

use crate::model::tests::chat;
use crate::settings::{self, LoosePages};
use crate::shell::Shell;
use crate::test_support::{
    click, draw, drawn, fingers, frames, open_page, pin, pressed, repo, select, shell_on,
    shell_on_chat, shell_with,
};

/// The shell over a checkout of its own for each of `projects`, named after `name`, in `count`
/// spaces, each project in the space it's paired with, those of the first there already: the
/// first space shown, its first project selected, motion reduced so what moves is where it goes
/// at once.
pub(super) fn in_spaces<const N: usize>(
    name: &str,
    (count, projects): (usize, [(&str, usize); N]),
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, [PathBuf; N]) {
    let roots = projects
        .map(|(project, _)| repo(&format!("{name}-{project}"), &[("README.md", "hi\n")]));
    let (window, shell) = shell_with(&roots.each_ref().map(PathBuf::as_path), cx);
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        settings::update(cx, |settings| {
            for _ in 1..count {
                settings.add_space();
            }
            for (root, (_, space)) in roots.iter().zip(projects) {
                if space > 0 {
                    settings.move_project(root, space);
                }
            }
        });
    });
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    select(&shell, &roots[0], cx);
    draw(window, cx);
    (window, shell, roots)
}

/// Space 0 holds `app` and `site`, space 1 `docs`, space 2 nothing and space 3 `blog`; space 0
/// is shown, `app` selected, motion reduced.
fn four_spaces(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, [PathBuf; 4]) {
    in_spaces(name, (4, [("app", 0), ("site", 0), ("docs", 1), ("blog", 3)]), cx)
}

fn space(cx: &TestAppContext) -> usize {
    cx.read(|cx| settings::get(cx).space)
}

fn offset(shell: &Entity<Shell>, cx: &TestAppContext) -> f32 {
    cx.read(|cx| shell.read(cx).swipe.offset)
}

/// The project shown, none in a space with no project, and the space shown.
pub(super) fn at(shell: &Entity<Shell>, cx: &TestAppContext) -> (Option<PathBuf>, usize) {
    let project = cx.read(|cx| (shell.read(cx).workspace.project()).map(|it| it.path.clone()));
    (project, space(cx))
}

/// Presses `key` with nothing focused, as when the window has just come forward: the project
/// and the space shown after, once the action bound to it ran.
fn press(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    key: &'static str,
    cx: &mut TestAppContext,
) -> (Option<PathBuf>, usize) {
    (cx.update_window(window, |_, window, cx| window.blur(cx))).unwrap();
    let (action, contexts) = pressed(window, key, cx);
    assert!(action.is_some(), "{key} ran nothing in {contexts:?}");
    at(shell, cx)
}

/// Which of the projects at `indices` in the workspace the sidebar lists, by their rows.
fn listed(window: AnyWindowHandle, indices: usize, cx: &mut TestAppContext) -> Vec<usize> {
    (0..indices)
        .filter(|index| drawn(window, format!("sidebar-project-{index}"), cx).is_some())
        .collect()
}

#[gpui_kit::test]
fn the_sidebar_lists_the_projects_of_the_space_shown(cx: &mut TestAppContext) {
    let (window, shell, _) = four_spaces("sidebar-space-rows", cx);
    assert_eq!(listed(window, 4, cx), [0, 1], "space 0's");
    let empty = |cx: &mut TestAppContext| drawn(window, "sidebar-add-project", cx).is_some();
    assert!(!empty(cx));
    for (shown, rows) in [(1, &[2][..]), (2, &[][..]), (3, &[3][..])] {
        shell.update(cx, |shell, cx| shell.show_space(shown, cx));
        assert_eq!(listed(window, 4, cx), rows, "space {shown}'s");
        assert_eq!(empty(cx), rows.is_empty(), "space {shown} offers to add a project");
    }
}

/// A mark shows its space, at the project it showed last.
#[gpui_kit::test]
fn a_mark_shows_its_space_at_the_project_it_showed_last(cx: &mut TestAppContext) {
    let (window, shell, [_, site, docs, _]) = four_spaces("sidebar-space-marks", cx);
    select(&shell, &site, cx);
    click(window, "space-mark-1", cx);
    assert_eq!(at(&shell, cx), (Some(docs), 1));
    click(window, "space-mark-2", cx);
    assert_eq!(at(&shell, cx), (None, 2), "a space with no project shows none");
    click(window, "space-mark-0", cx);
    assert_eq!(at(&shell, cx), (Some(site), 0), "the one it showed last");
}

/// ⌥⇧ and ⌘⌥⇧ arrows show the space beside, at the project it showed last; past the first or
/// last space the space gives toward the side with none and springs back, showing nothing new.
#[gpui_kit::test]
fn shift_option_arrows_show_the_space_beside_and_nudge_at_the_ends(cx: &mut TestAppContext) {
    let (window, shell, [app, _, docs, blog]) = four_spaces("sidebar-space-keys", cx);
    let go = |key, cx: &mut TestAppContext| press(window, &shell, key, cx);
    assert_eq!(go("alt-shift-right", cx), (Some(docs.clone()), 1));
    assert_eq!(go("cmd-alt-shift-right", cx), (None, 2));
    assert_eq!(go("alt-shift-right", cx), (Some(blog.clone()), 3));
    assert_eq!(go("cmd-alt-shift-left", cx), (None, 2));
    assert_eq!(go("alt-shift-left", cx), (Some(docs), 1));
    assert_eq!(go("alt-shift-left", cx), (Some(app.clone()), 0));

    cx.update(|cx| cx.set_reduce_motion(false));
    assert_eq!(go("alt-shift-left", cx), (Some(app.clone()), 0), "nothing before the first");
    frames(window, 3, cx);
    assert!(offset(&shell, cx) > 0., "gives toward the side with none");
    frames(window, 60, cx);
    assert_eq!((at(&shell, cx), offset(&shell, cx)), ((Some(app), 0), 0.));

    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, |shell, cx| shell.show_space(3, cx));
    cx.update(|cx| cx.set_reduce_motion(false));
    assert_eq!(go("cmd-alt-shift-right", cx), (Some(blog.clone()), 3), "nothing past the last");
    frames(window, 3, cx);
    assert!(offset(&shell, cx) < 0., "gives toward the side with none");
    frames(window, 60, cx);
    assert_eq!((at(&shell, cx), offset(&shell, cx)), ((Some(blog), 3), 0.));
}

/// ⌥← and ⌥→ step the project rows, on into the space beside past the first or last, passing
/// over a space with no project, and give at either end.
#[gpui_kit::test]
fn option_arrows_step_the_project_rows_on_into_the_space_beside(cx: &mut TestAppContext) {
    let (window, shell, [app, site, docs, blog]) = four_spaces("sidebar-space-rows-keys", cx);
    let go = |key, cx: &mut TestAppContext| press(window, &shell, key, cx);
    assert_eq!(go("alt-right", cx), (Some(site.clone()), 0), "the next row");
    assert_eq!(go("alt-right", cx), (Some(docs.clone()), 1), "the next space's");
    assert_eq!(go("cmd-alt-right", cx), (Some(blog), 3), "past the empty space");
    assert_eq!(go("alt-left", cx), (Some(docs), 1));
    assert_eq!(go("cmd-alt-left", cx), (Some(site), 0), "the space before's last");
    assert_eq!(go("alt-left", cx), (Some(app.clone()), 0));

    cx.update(|cx| cx.set_reduce_motion(false));
    assert_eq!(go("alt-left", cx), (Some(app), 0), "nothing before the first");
    frames(window, 3, cx);
    assert!(offset(&shell, cx) > 0., "gives toward the side with none");
    frames(window, 60, cx);
    assert!(offset(&shell, cx).abs() < f32::EPSILON, "and springs back");
}

/// What's shown: "pinned", "loose", or the title of the checkout's tab.
fn tab_shown(shell: &Entity<Shell>, cx: &TestAppContext) -> String {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        if workspace.pinned_shown().is_some() {
            return "pinned".into();
        }
        if workspace.loose_shown().is_some() {
            return "loose".into();
        }
        let tab = (workspace.shown_checkout_tab()).and_then(|_| workspace.tab_shown());
        tab.map_or_else(String::new, |tab| tab.first().title.to_string())
    })
}

/// ⌥↑ and ⌥↓ step the tabs in the order the rail lists them: the pinned page, the loose page,
/// then the selected branch's tabs, left to right; at either end the space gives and springs
/// back.
#[gpui_kit::test]
fn option_up_and_down_step_the_tabs_in_the_order_the_rail_lists_them(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("sidebar-tab-keys", cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    pin(window, &shell, &["https://pinned.example/"], cx);
    let page = open_page(window, &shell, Some("https://loose.example/"), cx);
    shell.update(cx, |shell, cx| {
        shell.loosen_tab_of(page, usize::MAX, cx);
        shell.workspace.push_tab(chat("one"));
        shell.workspace.push_tab(chat("two"));
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    draw(window, cx);
    let go = |key, cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| window.blur(cx))).unwrap();
        pressed(window, key, cx);
        tab_shown(&shell, cx)
    };
    assert_eq!(tab_shown(&shell, cx), "one");
    let up: Vec<_> = (0..3).map(|_| go("alt-up", cx)).collect();
    assert_eq!(up, ["loose", "pinned", "pinned"], "back along the rail, to the first");
    let down: Vec<_> = (0..4).map(|_| go("alt-down", cx)).collect();
    assert_eq!(down, ["loose", "one", "two", "two"], "on along the rail, to the last");

    cx.update(|cx| cx.set_reduce_motion(false));
    assert_eq!(go("alt-down", cx), "two", "nothing past the last");
    frames(window, 3, cx);
    assert!(offset(&shell, cx) < 0., "gives toward the side with none");
    frames(window, 60, cx);
    assert!(offset(&shell, cx).abs() < f32::EPSILON, "and springs back");
}

/// Where the rail's pieces are drawn: the pinned pages' pill, the loose page's tab, the
/// checkout's tab and its Changes button.
fn rail(window: AnyWindowHandle, cx: &mut TestAppContext) -> [Bounds<Pixels>; 4] {
    ["rail-pinned", "rail-unpinned-0", "tab-0-0-0", "toggle-changes-0-0"]
        .map(|piece| drawn(window, piece, cx).unwrap_or_else(|| panic!("{piece} is drawn")))
}

/// How far across each of the rail's pieces is from where it rests while the space is `pulled`
/// that far, the loose pages listed as `pages` says.
fn moved(
    (window, shell): (AnyWindowHandle, &Entity<Shell>),
    pages: LoosePages,
    pulled: f32,
    cx: &mut TestAppContext,
) -> [f32; 4] {
    cx.update(|cx| settings::update(cx, |settings| settings.loose_pages = pages));
    let rest = rail(window, cx);
    shell.update(cx, |shell, cx| {
        shell.swipe.offset = pulled;
        cx.notify();
    });
    let now = rail(window, cx);
    shell.update(cx, |shell, cx| {
        shell.swipe.offset = 0.;
        cx.notify();
    });
    draw(window, cx);
    [0, 1, 2, 3].map(|piece| (now[piece].left() - rest[piece].left()).as_f32())
}

/// As the space moves, the checkout's tabs and its Changes and Files buttons move with it, and
/// the loose pages while each space lists its own; the pinned pages, and the loose pages every
/// space shares, stay put.
#[gpui_kit::test]
fn the_rails_pieces_of_the_space_move_with_it(cx: &mut TestAppContext) {
    let (window, shell) = shell_on_chat("rail-slide", cx);
    pin(window, &shell, &["https://pinned.example/"], cx);
    let page = open_page(window, &shell, Some("https://loose.example/"), cx);
    shell.update(cx, |shell, cx| {
        shell.loosen_tab_of(page, usize::MAX, cx);
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    draw(window, cx);
    let near = |got: [f32; 4], by: [f32; 4]| {
        got.iter().zip(by).all(|(got, by)| (got - by).abs() < 0.5)
    };
    let shared = moved((window, &shell), LoosePages::Shared, 40., cx);
    assert!(near(shared, [0., 0., 40., 40.]), "shared: {shared:?}");
    let own = moved((window, &shell), LoosePages::PerSpace, -40., cx);
    assert!(near(own, [0., -40., -40., -40.]), "per space: {own:?}");
}

/// Two fingers across the sidebar drag the space; with reduced motion, let go far enough, the
/// next space is shown at once, at rest, with no frames to wait for.
#[gpui_kit::test]
fn with_reduced_motion_fingers_let_go_show_the_next_space_at_once(cx: &mut TestAppContext) {
    let (window, shell, [_, _, docs, _]) = four_spaces("sidebar-space-swipe", cx);
    fingers(window, TouchPhase::Started, 0., cx);
    for _ in 0..4 {
        fingers(window, TouchPhase::Moved, -40., cx);
    }
    assert!(offset(&shell, cx) < -100., "the space follows the fingers");
    assert_eq!(space(cx), 0);
    fingers(window, TouchPhase::Ended, 0., cx);
    assert_eq!((at(&shell, cx), offset(&shell, cx)), ((Some(docs), 1), 0.));
}

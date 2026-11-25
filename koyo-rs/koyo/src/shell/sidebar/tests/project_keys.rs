//! ⌥← and ⌥→ show the project to the left and to the right (`docs/sidebar.md` 6): along the
//! space's projects, and past its first or last on into the space beside, at the project
//! nearest, passing over a space with no project. Pressed again while the space slides, they go
//! on from the project it's bringing in. ⌥⇧← and ⌥⇧→ show the space beside, at the project it
//! showed last.

use std::path::PathBuf;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::space_keys::{PROJECTS_MOVE, SPACES_MOVE, space};
use super::{act, sidebar_shell};
use crate::shell::spaces::NewSpace;
use crate::projects::{MoveProject, MoveProjectToNewSpace};
use crate::settings;
use crate::shell::Shell;
use crate::test_support::{draw, frames, open_page, pressed};

/// The project shown, none in a space with no project, and the space shown.
fn at(shell: &Entity<Shell>, cx: &TestAppContext) -> (Option<PathBuf>, usize) {
    let project = cx.read(|cx| (shell.read(cx).workspace.project()).map(|it| it.path.clone()));
    (project, space(cx))
}

/// Presses `key` with nothing focused, as when the window has just come forward: the project
/// and the space shown after.
fn press(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    key: &'static str,
    cx: &mut TestAppContext,
) -> (Option<PathBuf>, usize) {
    (cx.update_window(window, |_, window, cx| window.blur(cx))).unwrap();
    let (action, _) = pressed(window, key, cx);
    let expected = match key {
        "alt-left" => PROJECTS_MOVE[0],
        "alt-right" => PROJECTS_MOVE[1],
        "alt-shift-left" => SPACES_MOVE[0],
        _ => SPACES_MOVE[1],
    };
    assert_eq!(action, expected, "{key}");
    at(shell, cx)
}

/// Space 0 holds `app` and `site`, space 1 `docs`, space 2 nothing and space 3 `blog`; space 0
/// is shown, `app` selected.
fn four_spaces(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, [PathBuf; 4]) {
    let names = ["app", "site", "docs", "blog"].map(|project| format!("{name}-{project}"));
    let (window, shell, roots) = sidebar_shell(&names.each_ref().map(String::as_str), cx);
    act(window, MoveProjectToNewSpace(roots[2].clone()), cx);
    act(window, NewSpace, cx);
    act(window, MoveProjectToNewSpace(roots[3].clone()), cx);
    shell.update(cx, |shell, cx| {
        shell.show_space(0, cx);
        let app = shell.workspace.project_index(&roots[0]).unwrap();
        shell.select_project(app, cx);
    });
    draw(window, cx);
    let roots: [PathBuf; 4] = roots.try_into().unwrap();
    assert_eq!(at(&shell, cx), (Some(roots[0].clone()), 0));
    (window, shell, roots)
}

#[gpui_kit::test]
fn option_arrows_go_along_the_projects_and_on_into_the_space_beside(cx: &mut TestAppContext) {
    let (window, shell, [app, site, docs, blog]) = four_spaces("project-keys-along", cx);
    let go = |key, cx: &mut TestAppContext| press(window, &shell, key, cx);
    assert_eq!(
        go("alt-right", cx),
        (Some(site.clone()), 0),
        "the next project"
    );
    assert_eq!(
        go("alt-right", cx),
        (Some(docs.clone()), 1),
        "the next space's"
    );
    assert_eq!(
        go("alt-right", cx),
        (Some(blog.clone()), 3),
        "past the empty space"
    );
    assert_eq!(
        go("alt-right", cx),
        (Some(blog), 3),
        "nothing past the last"
    );
    assert_eq!(go("alt-left", cx), (Some(docs), 1));
    assert_eq!(
        go("alt-left", cx),
        (Some(site), 0),
        "the space before's last"
    );
    assert_eq!(go("alt-left", cx), (Some(app.clone()), 0));
    assert_eq!(
        go("alt-left", cx),
        (Some(app), 0),
        "nothing before the first"
    );
}

#[gpui_kit::test]
fn shift_option_arrows_show_the_space_beside_at_the_project_it_showed_last(
    cx: &mut TestAppContext,
) {
    let (window, shell, [app, site, docs, blog]) = four_spaces("project-keys-spaces", cx);
    let go = |key, cx: &mut TestAppContext| press(window, &shell, key, cx);
    assert_eq!(go("alt-shift-right", cx), (Some(docs.clone()), 1));
    assert_eq!(go("alt-shift-right", cx), (None, 2), "the empty space");
    assert_eq!(go("alt-right", cx), (Some(blog), 3));
    assert_eq!(go("alt-shift-left", cx), (None, 2));
    assert_eq!(go("alt-left", cx), (Some(docs.clone()), 1));
    assert_eq!(go("alt-shift-left", cx), (Some(app), 0));
    assert_eq!(go("alt-shift-right", cx), (Some(docs), 1));
    assert_eq!(go("alt-left", cx), (Some(site), 0));
}

/// As picking a project does, ⌥→ leaves a pinned page shown for the project's tab, whether the
/// project is in the space shown or the next.
#[gpui_kit::test]
fn option_arrows_leave_a_pinned_page_for_the_project(cx: &mut TestAppContext) {
    let (window, shell, [_, site, docs, _]) = four_spaces("project-keys-pinned", cx);
    let pinned = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.pinned_shown());
    let key = open_page(window, &shell, Some("https://example.com/inbox"), cx);
    shell.update(cx, |shell, cx| assert!(shell.pin_tab_of(key, usize::MAX, cx)));
    draw(window, cx);
    assert!(pinned(cx).is_some(), "the pinned page shows");
    assert_eq!(press(window, &shell, "alt-right", cx), (Some(site), 0));
    assert!(pinned(cx).is_none(), "left for the next project");
    shell.update(cx, |shell, cx| {
        shell.show_tab_holding(key, cx);
    });
    draw(window, cx);
    assert!(pinned(cx).is_some(), "the pinned page shows again");
    assert_eq!(press(window, &shell, "alt-right", cx), (Some(docs), 1));
    assert!(pinned(cx).is_none(), "left for the next space's project");
}

/// With motion, the space slides: pressed again on the way, ⌥→ goes on from the project coming
/// in, and ⌥← back the other way brings the space shown back to rest.
#[gpui_kit::test]
fn pressed_again_while_the_space_slides_they_go_on_from_the_project_coming_in(
    cx: &mut TestAppContext,
) {
    let names = [
        "project-keys-slide-app",
        "project-keys-slide-docs",
        "project-keys-slide-blog",
    ];
    let (window, shell, roots) = sidebar_shell(&names, cx);
    let [app, docs, blog]: [PathBuf; 3] = roots.try_into().unwrap();
    act(window, MoveProjectToNewSpace(docs.clone()), cx);
    let made = cx.read(|cx| settings::get(cx).shown_space_id());
    act(window, MoveProject(blog.clone(), made), cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);
    assert_eq!(at(&shell, cx), (Some(app.clone()), 0));
    cx.update(|cx| cx.set_reduce_motion(false));
    let go = |key, cx: &mut TestAppContext| press(window, &shell, key, cx);
    let offset = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).swipe.offset);

    assert_eq!(go("alt-right", cx), (Some(app.clone()), 0), "on its way");
    assert_eq!(go("alt-right", cx), (Some(app), 0), "still on its way");
    frames(window, 60, cx);
    assert_eq!((at(&shell, cx), offset(cx)), ((Some(blog.clone()), 1), 0.));

    assert_eq!(
        go("alt-left", cx),
        (Some(docs.clone()), 1),
        "the project before, at once"
    );
    assert_eq!(
        go("alt-left", cx),
        (Some(docs.clone()), 1),
        "space 0 on its way"
    );
    frames(window, 3, cx);
    assert!(offset(cx) > 0., "sliding out the right");
    assert_eq!(go("alt-right", cx), (Some(docs.clone()), 1), "turned back");
    frames(window, 60, cx);
    assert_eq!((at(&shell, cx), offset(cx)), ((Some(docs), 1), 0.));

    assert_eq!(go("alt-right", cx), (Some(blog.clone()), 1));
    assert_eq!(
        go("alt-right", cx),
        (Some(blog.clone()), 1),
        "nothing past it"
    );
    frames(window, 3, cx);
    assert!(offset(cx) < 0., "gives toward the side with none");
    frames(window, 60, cx);
    assert_eq!((at(&shell, cx), offset(cx)), ((Some(blog), 1), 0.));
}

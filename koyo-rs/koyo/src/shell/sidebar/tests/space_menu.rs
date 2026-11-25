//! A project's Move to Space and a space's menu in the one layout (`docs/layout.md`, Moving a
//! project to another space, Space menu and theme): each acts on the space it was opened for,
//! known by its id wherever the spaces have moved since; a project moved shows in its new space,
//! and the last space is never deleted.

use std::path::{Path, PathBuf};

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::{act, shown};
use crate::shell::spaces::{DeleteSpace, RenameSpace, delete};
use crate::files::tests::dialog;
use crate::modal;
use crate::projects::{MoveProject, MoveProjectToNewSpace};
use crate::settings;
use crate::shell::Shell;
use crate::test_support::{draw, press, repo, select, shell_with};

/// The shell in the one layout with a project for each of `names`, the first selected, motion
/// reduced so a space shows at once.
fn one_layout(
    names: &[&str],
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, Vec<PathBuf>) {
    let roots: Vec<_> = (names.iter())
        .map(|name| repo(name, &[("README.md", "hi\n")]))
        .collect();
    let refs: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
    let (window, shell) = shell_with(&refs, cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    select(&shell, &roots[0], cx);
    draw(window, cx);
    (window, shell, roots)
}

/// The id of the space at `at`.
pub(super) fn id(at: usize, cx: &TestAppContext) -> u64 {
    cx.read(|cx| settings::get(cx).spaces[at].id)
}

/// Each space's projects, in the order the spaces are listed.
fn spaces(cx: &TestAppContext) -> Vec<Vec<PathBuf>> {
    cx.read(|cx| {
        (settings::get(cx).spaces.iter())
            .map(|space| space.projects.clone())
            .collect()
    })
}

/// The spaces' names, in their order.
fn names(cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        (settings::get(cx).spaces.iter())
            .map(|space| space.name.clone())
            .collect()
    })
}

/// Moves the space at `from` to the place `to`, as its mark let go there does.
fn move_space(from: usize, to: usize, window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update(|cx| settings::update(cx, |settings| settings.move_space(from, to)));
    draw(window, cx);
}

#[gpui_kit::test]
fn new_space_makes_a_space_and_moves_the_project_into_it_shown(cx: &mut TestAppContext) {
    let projects = ["space-menu-new-app", "space-menu-new-site"];
    let (window, shell, roots) = one_layout(&projects, cx);
    let [app, site]: [PathBuf; 2] = roots.try_into().unwrap();
    act(window, MoveProjectToNewSpace(site.clone()), cx);
    assert_eq!(spaces(cx), [vec![app], vec![site.clone()]]);
    assert_eq!(names(cx), ["Space 1", "Space 2"]);
    assert_eq!(shown(&shell, cx), (site, 1), "the project, in its new space");
}

#[gpui_kit::test]
fn move_to_space_moves_the_project_to_the_space_picked_and_shows_it(cx: &mut TestAppContext) {
    let projects = [
        "space-menu-move-app",
        "space-menu-move-site",
        "space-menu-move-docs",
    ];
    let (window, shell, roots) = one_layout(&projects, cx);
    let [app, site, docs]: [PathBuf; 3] = roots.try_into().unwrap();
    act(window, MoveProjectToNewSpace(docs.clone()), cx);
    let second = id(1, cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);

    act(window, MoveProject(site.clone(), second), cx);
    assert_eq!(
        spaces(cx),
        [vec![app.clone()], vec![site.clone(), docs.clone()]]
    );
    assert_eq!(
        shown(&shell, cx),
        (site.clone(), 1),
        "the space it went to, showing it"
    );

    move_space(1, 0, window, cx);
    act(window, MoveProject(app.clone(), second), cx);
    assert_eq!(
        spaces(cx),
        [vec![app.clone(), site, docs], vec![]],
        "the space picked, wherever it has moved"
    );
    assert_eq!(shown(&shell, cx), (app, 0));
}

#[gpui_kit::test]
fn rename_space_renames_the_space_it_was_opened_for_not_the_one_shown(cx: &mut TestAppContext) {
    let projects = ["space-menu-rename-app", "space-menu-rename-site"];
    let (window, shell, roots) = one_layout(&projects, cx);
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    let other = id(1, cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    move_space(1, 0, window, cx);

    act(window, RenameSpace(other), cx);
    let asked = cx.read(modal::window).expect("it asks for the name");
    draw(asked, cx);
    cx.simulate_input(asked, "work");
    press(asked, "enter", cx);
    cx.run_until_parked();
    assert_eq!(names(cx), ["work", "Space 1"]);
    let shown_name =
        cx.read(|cx| (settings::get(cx).shown_space()).map(|space| space.name.clone()));
    assert_eq!(
        shown_name.as_deref(),
        Some("Space 1"),
        "the space shown keeps its name"
    );
}

#[gpui_kit::test]
fn delete_space_deletes_the_space_it_was_opened_for_not_the_one_shown(cx: &mut TestAppContext) {
    let projects = ["space-menu-delete-app", "space-menu-delete-site"];
    let (window, shell, roots) = one_layout(&projects, cx);
    let [app, site]: [PathBuf; 2] = roots.try_into().unwrap();
    act(window, MoveProjectToNewSpace(site.clone()), cx);
    let (first, other) = (id(0, cx), id(1, cx));
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    move_space(1, 0, window, cx);

    act(window, DeleteSpace(other), cx);
    let (asked_window, asked) =
        dialog::<delete::DeleteSpace>(cx).expect("it asks first");
    (cx.update_window(asked_window, |_, window, cx| {
        asked.update(cx, |asked, cx| asked.delete(window, cx));
    }))
    .unwrap();
    cx.run_until_parked();
    assert_eq!(id(0, cx), first, "the space it was opened for went");
    assert_eq!(
        spaces(cx),
        [vec![app.clone(), site]],
        "its project moved to the other space"
    );
    assert_eq!(shown(&shell, cx), (app, 0), "the space shown stays shown");
}

#[gpui_kit::test]
fn delete_space_does_nothing_with_one_space_left(cx: &mut TestAppContext) {
    let (window, _, _) = one_layout(&["space-menu-last"], cx);
    let only = id(0, cx);
    act(window, DeleteSpace(only), cx);
    assert!(cx.read(modal::window).is_none(), "nothing to ask");
    assert_eq!(names(cx), ["Space 1"]);
}

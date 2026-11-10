//! A project's tile dragged to another slot of its space's grid moves its project there
//! (`docs/arc-sidebar.md` 3), across rows too; the order is the sidebar's, saved, and the
//! projects of other spaces keep their places in it.

use std::path::PathBuf;

use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::{act, arc_shell, drag};
use crate::arc::actions::MoveProjectToNewSpace;
use crate::settings;
use crate::shell::Shell;
use crate::test_support::draw;

/// The projects in the sidebar's order, as the settings keep them, by their folders' names.
fn saved(cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| names(&settings::get(cx).projects))
}

fn names(paths: &[PathBuf]) -> Vec<String> {
    (paths.iter())
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            // Without the test's own prefix and the process's number.
            let name = name.trim_start_matches("arc-tiles-");
            name.split('-').next().unwrap().to_owned()
        })
        .collect()
}

/// The tiles of the space shown, in the order they're drawn, by their projects' names.
fn tiles(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let shell = shell.read(cx);
        let paths: Vec<PathBuf> = (shell.space_projects(cx).into_iter())
            .map(|index| shell.workspace.projects[index].path.clone())
            .collect();
        names(&paths)
    })
}

#[gpui_kit::test]
fn a_tile_dragged_to_another_slot_moves_its_project_there(cx: &mut TestAppContext) {
    let projects = ["a", "x", "b", "c", "d", "e"].map(|name| format!("arc-tiles-{name}"));
    let projects: Vec<&str> = projects.iter().map(String::as_str).collect();
    let (window, shell, roots) = arc_shell(&projects, cx);
    // `x` is another space's, in the middle of the sidebar's order.
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    shell.update(cx, |shell, cx| {
        shell.show_space(0, cx);
        // `b` is the project shown.
        shell.select_project(2, cx);
    });
    draw(window, cx);
    assert_eq!(tiles(&shell, cx), ["a", "b", "c", "d", "e"]);
    let selected = |cx: &TestAppContext| {
        cx.read(|cx| {
            let project = shell.read(cx).workspace.project().expect("a project");
            names(std::slice::from_ref(&project.path)).remove(0)
        })
    };

    // Five tiles are three to a row: the first, dragged onto the last, crosses to the row below.
    drag(window, "arc-tile-0", "arc-tile-5", cx);
    assert_eq!(tiles(&shell, cx), ["b", "c", "d", "e", "a"]);
    assert_eq!(
        saved(cx),
        ["b", "x", "c", "d", "e", "a"],
        "saved, the other space's project where it was"
    );
    assert_eq!(selected(cx), "b", "the project selected stays selected");
    cx.read(|cx| {
        let spaces = &settings::get(cx).spaces;
        assert_eq!(names(&spaces[0].projects), ["b", "c", "d", "e", "a"]);
        assert_eq!(names(&spaces[1].projects), ["x"]);
    });
    let listed = cx.read(|cx| names(&shell.read(cx).workspace.paths()));
    assert_eq!(listed, saved(cx), "as the Orca list shows them");

    // And back up a row, into the middle: `e`, in the workspace's fifth place, onto `c`.
    drag(window, "arc-tile-4", "arc-tile-2", cx);
    assert_eq!(tiles(&shell, cx), ["b", "e", "c", "d", "a"]);
    assert_eq!(saved(cx), ["b", "x", "e", "c", "d", "a"]);

    // A press and release without a move still selects.
    let mut vcx = VisualTestContext::from_window(window, cx);
    let at = vcx.debug_bounds("arc-tile-5").expect("a's tile").center();
    vcx.simulate_click(at, Modifiers::default());
    draw(window, cx);
    assert_eq!(selected(cx), "a");
    assert_eq!(tiles(&shell, cx), ["b", "e", "c", "d", "a"], "nothing moved");
}

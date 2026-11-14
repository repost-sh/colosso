//! ⌥← and ⌥→ show the project to the left and to the right (`docs/arc-sidebar.md` 7), as its
//! tile would: along the space's tiles, and past its first or last on into the space beside, at
//! the project nearest, passing over a space with no project. Pressed again while the space
//! slides, they go on from the project it's bringing in. Along the tiles, the selected tile's
//! light passes to the next: out of the one toward the other, and into that one from the side it
//! came from, together. ⌥⇧← and ⌥⇧→ show the space beside, at the project it showed last.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{AnyWindowHandle, AppContext as _, Axis, Entity, TestAppContext};

use super::space_keys::{PROJECTS_MOVE, SPACES_MOVE, pressed, space};
use super::{act, arc_shell, drawn};
use crate::arc::actions::{MoveProject, MoveProjectToNewSpace, NewSpace};
use crate::shell::Shell;
use crate::test_support::{draw, open_page};

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

/// `count` frames of the display's, 16 ms apart, each drawn: a space's slide moving on.
pub(super) fn frames(window: AnyWindowHandle, count: usize, cx: &mut TestAppContext) {
    for _ in 0..count {
        cx.executor().advance_clock(Duration::from_millis(16));
        (cx.update_window(window, |_, window, cx| window.simulate_next_frame(cx))).unwrap();
        draw(window, cx);
    }
}

/// The part of what's drawn as `place` that the passing light drawn as `light` covers (`glide`),
/// along `along`: where it starts and ends, as shares of the place's length; none while there's
/// none.
pub(super) fn passing(
    window: AnyWindowHandle,
    (light, place): (String, String),
    along: Axis,
    cx: &mut TestAppContext,
) -> Option<(f32, f32)> {
    let light = drawn(window, light, cx)?;
    let place = drawn(window, place, cx).expect("the place the light passes");
    let (start, length, ends) = match along {
        Axis::Horizontal => (
            place.left(),
            place.size.width,
            (light.left(), light.right()),
        ),
        Axis::Vertical => (
            place.top(),
            place.size.height,
            (light.top(), light.bottom()),
        ),
    };
    let share = |at: gpui_kit::Pixels| (at - start) / length;
    Some((share(ends.0), share(ends.1)))
}

/// The light caught on its way from `going` to `coming`, the keys going `on` or back: out of the
/// end of the one it leaves and in at the start of the one it comes to (going back, the other way
/// about), as far in the one as it's gone out of the other.
pub(super) fn assert_passing(going: Option<(f32, f32)>, coming: Option<(f32, f32)>, on: bool) {
    // Going back is going on, seen from the other end.
    let turn = |(from, to): (f32, f32)| if on { (from, to) } else { (1. - to, 1. - from) };
    let (out, end) = turn(going.expect("the light leaving"));
    let (start, into) = turn(coming.expect("the light coming in"));
    assert!(
        out > 0.05 && (end - 1.).abs() < 0.01,
        "out of the end: {going:?}"
    );
    assert!(
        start.abs() < 0.01 && into < 0.95,
        "in at the start: {coming:?}"
    );
    assert!((out - into).abs() < 0.01, "together: {going:?} {coming:?}");
}

/// Space 0 holds `app` and `site`, space 1 `docs`, space 2 nothing and space 3 `blog`; space 0
/// is shown, `app` selected.
fn four_spaces(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, [PathBuf; 4]) {
    let names = ["app", "site", "docs", "blog"].map(|project| format!("{name}-{project}"));
    let (window, shell, roots) = arc_shell(&names.each_ref().map(String::as_str), cx);
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
fn option_arrows_go_along_the_tiles_and_on_into_the_space_beside(cx: &mut TestAppContext) {
    let (window, shell, [app, site, docs, blog]) = four_spaces("project-keys-along", cx);
    let go = |key, cx: &mut TestAppContext| press(window, &shell, key, cx);
    assert_eq!(
        go("alt-right", cx),
        (Some(site.clone()), 0),
        "the next tile"
    );
    assert_eq!(
        go("alt-right", cx),
        (Some(docs.clone()), 1),
        "the next space's"
    );
    // Space 2 holds no project, so it's passed over.
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
    // Back into space 0, at its last tile, the nearest.
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
    // From a space with no project, ⌥← and ⌥→ go to the nearest project that way.
    assert_eq!(go("alt-right", cx), (Some(blog), 3));
    assert_eq!(go("alt-shift-left", cx), (None, 2));
    assert_eq!(go("alt-left", cx), (Some(docs.clone()), 1));
    // Space 0 showed `app` last: ⌥⇧← comes back to it, where ⌥← from `docs` goes to `site`.
    assert_eq!(go("alt-shift-left", cx), (Some(app), 0));
    assert_eq!(go("alt-shift-right", cx), (Some(docs), 1));
    assert_eq!(go("alt-left", cx), (Some(site), 0));
}

/// As a project's tile does, ⌥→ leaves a pinned page shown for the project's tab, whether the
/// project is in the space shown or the next.
#[gpui_kit::test]
fn option_arrows_leave_a_pinned_page_for_the_project(cx: &mut TestAppContext) {
    let (window, shell, [_, site, docs, _]) = four_spaces("project-keys-pinned", cx);
    let pinned = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.pinned_shown());
    let key = open_page(window, &shell, Some("https://example.com/inbox"), cx);
    shell.update(cx, |shell, cx| assert!(shell.pin_tab_of(key, cx)));
    draw(window, cx);
    assert!(pinned(cx).is_some(), "the pinned page shows");
    assert_eq!(press(window, &shell, "alt-right", cx), (Some(site), 0));
    assert!(pinned(cx).is_none(), "left for the next tile's project");
    shell.update(cx, |shell, cx| shell.pick_pinned(key, cx));
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
    let (window, shell, roots) = arc_shell(&names, cx);
    let [app, docs, blog]: [PathBuf; 3] = roots.try_into().unwrap();
    act(window, MoveProjectToNewSpace(docs.clone()), cx);
    act(window, MoveProject(blog.clone(), 1), cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);
    assert_eq!(at(&shell, cx), (Some(app.clone()), 0));
    cx.update(|cx| cx.set_reduce_motion(false));
    let go = |key, cx: &mut TestAppContext| press(window, &shell, key, cx);
    let offset = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).swipe.offset);

    // Space 1 is on its way, `docs` with it, until it's half-way in: ⌥→ again goes on to `blog`.
    assert_eq!(go("alt-right", cx), (Some(app.clone()), 0), "on its way");
    assert_eq!(go("alt-right", cx), (Some(app), 0), "still on its way");
    frames(window, 60, cx);
    assert_eq!((at(&shell, cx), offset(cx)), ((Some(blog.clone()), 1), 0.));

    // `docs` is in the space shown, so at once; then `app`'s space starts in, and ⌥→ turns it
    // back, the space shown coming back to rest on `docs`.
    assert_eq!(
        go("alt-left", cx),
        (Some(docs.clone()), 1),
        "the tile before, at once"
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

    // Past the last project of all, the space gives and settles back.
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

/// With motion, ⌥→ passes the light from `app`'s tile to `site`'s beside it: out of `app`'s right
/// and into `site`'s left, as far in the one as it's gone out of the other, then the light is the
/// tile's own again. ⌥← passes it back the other way about.
#[gpui_kit::test]
fn along_the_tiles_the_light_passes_out_of_the_one_and_into_the_next(cx: &mut TestAppContext) {
    let (window, shell, [app, site, ..]) = four_spaces("project-keys-light", cx);
    let index = |path: &PathBuf, cx: &TestAppContext| {
        cx.read(|cx| shell.read(cx).workspace.project_index(path).unwrap())
    };
    let (app, site) = (index(&app, cx), index(&site, cx));
    let light = |index: usize, cx: &mut TestAppContext| {
        let drawn_as = (
            format!("arc-tile-light-{index}"),
            format!("arc-tile-{index}"),
        );
        passing(window, drawn_as, Axis::Horizontal, cx)
    };
    let both = |cx: &mut TestAppContext| (light(app, cx), light(site, cx));
    cx.update(|cx| cx.set_reduce_motion(false));
    assert_eq!(both(cx), (None, None));

    press(window, &shell, "alt-right", cx);
    frames(window, 3, cx);
    let (going, coming) = both(cx);
    assert_passing(going, coming, true);
    frames(window, 30, cx);
    assert_eq!(both(cx), (None, None));

    press(window, &shell, "alt-left", cx);
    frames(window, 3, cx);
    let (coming, going) = both(cx);
    assert_passing(going, coming, false);
    frames(window, 30, cx);
    assert_eq!(both(cx), (None, None));
}

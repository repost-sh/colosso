//! Viewers in the workspace's tabs: a picture opens as a picture where a file's text would,
//! the pointer zooms it about itself, a drawing shows its text on asking, a file written
//! again is shown again, and a link to a file opens it.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui_kit::{
    Action as _, AnyWindowHandle, AppContext as _, Entity, Modifiers, PinchEvent, ScrollDelta,
    ScrollEvent, TestAppContext, TouchPhase, VisualTestContext, point, px,
};
use image::{Rgba, RgbaImage};

use super::view::{State, Viewer};
use crate::files::tests::{checkout, shell_with_tree, tabs, with_shell};
use crate::files::{At, FilePane, OpenLinked, Place};
use crate::shell::Shell;

/// A picture `width` by `height` at `path` under `root`, grey with a see-through corner.
fn picture(root: &Path, path: &str, width: u32, height: u32) {
    let picture = RgbaImage::from_fn(width, height, |x, y| {
        if x < 4 && y < 4 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([128, 128, 128, 255])
        }
    });
    picture.save(root.join(path)).unwrap();
}

/// The file the shown tab shows.
fn shown_file(shell: &Entity<Shell>, cx: &TestAppContext) -> Entity<FilePane> {
    cx.read(|cx| shell.read(cx).viewing())
        .expect("a file shown")
}

fn shown_viewer(shell: &Entity<Shell>, cx: &TestAppContext) -> Entity<Viewer> {
    let file = shown_file(shell, cx);
    cx.read(|cx| file.read(cx).viewer().cloned())
        .expect("a viewer")
}

/// Runs what's pending, drawing `window` each time, until `done` says so: a viewer's thread
/// reads and draws on its own, outside the test's executor.
fn until(window: AnyWindowHandle, cx: &mut TestAppContext, done: impl Fn(&TestAppContext) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        (cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))).unwrap();
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "gave up waiting");
        #[expect(
            clippy::disallowed_methods,
            reason = "the viewer's thread isn't the test's to wait on"
        )]
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn open(path: &str, window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) {
    with_shell(window, shell, cx, |shell, window, cx| {
        shell.preview_file(Place::file(path), window, cx);
    });
}

fn zoom(viewer: &Entity<Viewer>, cx: &TestAppContext) -> (f32, bool) {
    cx.read(|cx| {
        let camera = viewer.read(cx).camera;
        (camera.zoom, camera.fit)
    })
}

#[gpui_kit::test]
fn a_picture_opens_as_a_picture_and_the_pointer_zooms_it_about_itself(cx: &mut TestAppContext) {
    let root = checkout("viewer-tab-picture");
    picture(&root, "shot.png", 300, 200);
    let (window, shell, _) = shell_with_tree(&root, cx);
    open("shot.png", window, &shell, cx);
    let viewer = shown_viewer(&shell, cx);
    until(window, cx, |cx| cx.read(|cx| viewer.read(cx).is_drawn()));
    assert_eq!(
        tabs(&shell, cx),
        ["[shot.png*]"],
        "previewed in a tab as any file is"
    );
    let file = shown_file(&shell, cx);
    assert!(
        cx.read(|cx| file.read(cx).editor().is_none()),
        "no editor over a picture"
    );
    assert!(cx.read(|cx| matches!(viewer.read(cx).state, State::Open)));
    // Smaller than the pane, it's shown at its own size, and has the keyboard.
    assert_eq!(zoom(&viewer, cx), (1., true));
    let focused = cx
        .update_window(window, |_, window, cx| file.read(cx).has_focus(window, cx))
        .unwrap();
    assert!(focused, "the viewer has the keyboard");

    let mut visual = VisualTestContext::from_window(window, cx);
    let pages = visual
        .debug_bounds("viewer-pages")
        .expect("the pages on screen");
    let middle = pages.center();
    // ⌘ and the wheel up zoom in, about the pointer.
    visual.simulate_event(ScrollEvent {
        position: middle,
        delta: ScrollDelta::Pixels(point(px(0.), px(200.))),
        modifiers: Modifiers::command(),
        ..ScrollEvent::default()
    });
    let (zoomed, fit) = zoom(&viewer, cx);
    assert!((zoomed - 2.).abs() < 0.01, "zoomed to {zoomed}");
    assert!(!fit, "zoomed, it no longer follows the pane");
    // A pinch out zooms out by as much as it says.
    visual.simulate_event(PinchEvent {
        position: middle,
        delta: -0.5,
        modifiers: Modifiers::none(),
        phase: TouchPhase::Moved,
    });
    let (pinched, _) = zoom(&viewer, cx);
    assert!((pinched - 1.).abs() < 0.01, "pinched to {pinched}");
    // Zoomed past the pane, the wheel alone moves the picture about rather than zooming.
    visual.simulate_event(ScrollEvent {
        position: middle,
        delta: ScrollDelta::Pixels(point(px(0.), px(600.))),
        modifiers: Modifiers::command(),
        ..ScrollEvent::default()
    });
    let (big, _) = zoom(&viewer, cx);
    assert!((big - 8.).abs() < 0.05, "zoomed to {big}");
    let scroll = |cx: &TestAppContext| cx.read(|cx| viewer.read(cx).camera.scroll);
    let before = scroll(cx);
    visual.simulate_event(ScrollEvent {
        position: middle,
        delta: ScrollDelta::Pixels(point(px(-30.), px(-40.))),
        ..ScrollEvent::default()
    });
    let after = scroll(cx);
    assert!(
        (zoom(&viewer, cx).0 - big).abs() < 0.001,
        "the wheel alone doesn't zoom"
    );
    assert_eq!(
        (after.0 - before.0, after.1 - before.1),
        (30., 40.),
        "it moves by the wheel's deltas"
    );
    // ⌘0 fits it again.
    visual.simulate_keystrokes("cmd-=");
    assert!(zoom(&viewer, cx).0 > 1.01, "⌘= zooms in");
    visual.simulate_keystrokes("cmd-0");
    assert_eq!(zoom(&viewer, cx), (1., true), "⌘0 fits it again");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_drawing_shows_its_text_on_asking_and_itself_again(cx: &mut TestAppContext) {
    let root = checkout("viewer-tab-drawing");
    std::fs::write(
        root.join("logo.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="32"><rect width="64" height="32"/></svg>"#,
    )
    .unwrap();
    let (window, shell, _) = shell_with_tree(&root, cx);
    open("logo.svg", window, &shell, cx);
    let viewer = shown_viewer(&shell, cx);
    until(window, cx, |cx| cx.read(|cx| viewer.read(cx).is_drawn()));
    let file = shown_file(&shell, cx);
    assert!(cx.read(|cx| !file.read(cx).shows_text()));
    // Its text, asked for, is made an editor of and shown, with the keyboard.
    (cx.update_window(window, |_, window, cx| {
        file.update(cx, |file, cx| file.show_text(true, window, cx));
    }))
    .unwrap();
    let text = |cx: &TestAppContext| {
        cx.read(|cx| {
            file.read(cx)
                .editor()
                .and_then(|editor| editor.read(cx).text(cx))
        })
    };
    until(window, cx, |cx| text(cx).is_some());
    assert!(cx.read(|cx| file.read(cx).shows_text()));
    let editor = cx.read(|cx| file.read(cx).editor().cloned()).unwrap();
    let text = text(cx).unwrap_or_default();
    assert!(text.starts_with("<svg"), "its text: {text}");
    let focused = (cx.update_window(window, |_, window, cx| {
        editor.read(cx).has_focus(window, cx)
    }))
    .unwrap();
    assert!(focused, "the editor has the keyboard");
    // And the drawing again, the same editor kept for next time.
    (cx.update_window(window, |_, window, cx| {
        file.update(cx, |file, cx| file.show_text(false, window, cx));
    }))
    .unwrap();
    cx.run_until_parked();
    assert!(cx.read(|cx| !file.read(cx).shows_text()));
    assert!(cx.read(|cx| file.read(cx).editor().is_some()));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_picture_written_again_is_shown_again(cx: &mut TestAppContext) {
    let root = checkout("viewer-tab-reread");
    picture(&root, "shot.png", 300, 200);
    let (window, shell, _) = shell_with_tree(&root, cx);
    open("shot.png", window, &shell, cx);
    let viewer = shown_viewer(&shell, cx);
    until(window, cx, |cx| cx.read(|cx| viewer.read(cx).is_drawn()));
    picture(&root, "shot.png", 120, 60);
    viewer.update(cx, Viewer::reread);
    let size = |cx: &TestAppContext| {
        cx.read(|cx| viewer.read(cx).probe.as_ref().map(|probe| probe.pixels))
    };
    until(window, cx, |cx| size(cx) == Some((120, 60)));
    until(window, cx, |cx| {
        cx.read(|cx| {
            (viewer.read(cx).picture.as_ref()).is_some_and(|picture| picture.size(0).width.0 == 120)
        })
    });
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_link_to_a_file_opens_it_in_a_tab(cx: &mut TestAppContext) {
    let root = checkout("viewer-tab-link");
    picture(&root, "shot.png", 30, 20);
    let (window, shell, _) = shell_with_tree(&root, cx);
    let open = |path: &Path, at: Option<At>, cx: &mut TestAppContext| {
        let path = path.to_path_buf();
        (cx.update_window(window, |_, window, cx| {
            window.dispatch_action(OpenLinked { path, at }.boxed_clone(), cx);
        }))
        .unwrap();
        cx.run_until_parked();
        tabs(&shell, cx)
    };
    assert_eq!(
        open(&root.join("src/lib.rs"), Some(At::Line(1)), cx),
        ["[lib.rs*]"]
    );
    assert_eq!(open(&root.join("shot.png"), None, cx), ["[shot.png*]"]);
    // A path to nothing opens nothing.
    assert_eq!(open(&root.join("gone.png"), None, cx), ["[shot.png*]"]);
    // A picture outside the checkout, as one pasted into a terminal is kept in the temporary
    // folder and Claude Code links its `[Image #1]` to it, opens too, shown as what it is.
    let elsewhere = crate::test_support::scratch("viewer-tab-link-pasted");
    picture(&elsewhere, "koyo-paste-1.png", 30, 20);
    assert_eq!(
        open(&elsewhere.join("koyo-paste-1.png"), None, cx),
        ["[koyo-paste-1.png*]"]
    );
    let viewer = shown_viewer(&shell, cx);
    until(window, cx, |cx| cx.read(|cx| viewer.read(cx).is_drawn()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&elsewhere);
}

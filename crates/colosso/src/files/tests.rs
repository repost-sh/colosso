use std::path::Path;
use std::time::{Duration, Instant};

use colosso_project::Feed;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, px, size};

use super::editor::language;
use super::*;
use crate::changes::Mark;
use crate::checkout::{Checkout, Content, read_at_most};
use crate::test_support::{git, in_window, scratch, write};
use crate::{right_panel, settings};

#[test]
fn files_are_read_as_text_unless_binary_or_too_large() {
    let root = scratch("files-read");
    write(&root, "text.rs", "fn main() {}\n".as_bytes());
    write(&root, "binary", b"\x7fELF\0\0\x01");
    write(&root, "large.txt", &[b'a'; 20]);
    let read = |name: &str| read_at_most(&root.join(name), 16);
    assert_eq!(read("text.rs"), Content::Text("fn main() {}\n".into()));
    assert_eq!(read("binary"), Content::Binary);
    assert_eq!(read("large.txt"), Content::TooLarge(20));
    assert!(matches!(read("missing"), Content::Failed(_)));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_file_is_read_once_however_many_views_show_it(cx: &mut TestAppContext) {
    let root = checkout("files-documents");
    let (window, panel) = open(&root, cx);
    let checkout = cx.read(|cx| panel.read(cx).checkout.clone());
    let (lib, readme) = (root.join("src/lib.rs"), root.join("README.md"));
    // However many views show a file, it's read once.
    let view = |path: &Path, cx: &mut TestAppContext| {
        let document = checkout.update(cx, |checkout, cx| checkout.document(path, cx));
        (cx.update_window(window, |_, window, cx| {
            cx.new(|cx| FileEditor::new(document, window, cx))
        }))
        .unwrap()
    };
    let (lib_editor, lib_again, readme_editor) =
        (view(&lib, cx), view(&lib, cx), view(&readme, cx));
    cx.read(|cx| {
        let document = |editor: &Entity<FileEditor>| editor.read(cx).document.clone();
        assert_eq!(document(&lib_editor), document(&lib_again));
        assert_ne!(document(&lib_editor), document(&readme_editor));
    });
    // Read off the main thread, then shown by each.
    cx.run_until_parked();
    for editor in [&lib_editor, &lib_again] {
        let text = cx.read(|cx| editor.read(cx).text(cx));
        assert_eq!(text.as_deref(), Some("pub fn a() {}\npub fn b() {}\n"));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_file_viewed_again_is_shown_in_the_editor_it_had(cx: &mut TestAppContext) {
    let root = checkout("files-viewed");
    let (window, panel) = open(&root, cx);
    let view = |path: &str, cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.view(Place::file(path), window, cx);
            });
        }))
        .unwrap();
        cx.read(|cx| {
            let viewed = panel.read(cx).viewing().unwrap();
            (viewed.path.clone(), viewed.editor.clone())
        })
    };
    let (lib, lib_editor) = view("src/lib.rs", cx);
    assert_eq!(lib, root.join("src/lib.rs"));
    let (readme, readme_editor) = view("README.md", cx);
    assert_eq!(readme, root.join("README.md"));
    assert_ne!(lib_editor, readme_editor);
    // Back to the first, by its full path: the same editor, and it's picked in the tree.
    let (again, again_editor) = view(root.join("src/lib.rs").to_str().unwrap(), cx);
    assert_eq!((again, again_editor), (lib, lib_editor));
    assert_eq!(selected(&panel, cx).as_deref(), Some("src/lib.rs"));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_place_puts_the_cursor_on_its_line_or_its_bytes(cx: &mut TestAppContext) {
    let root = checkout("files-place");
    let (window, panel) = open(&root, cx);
    let go = |at: At, cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| {
            let place = Place {
                path: "src/lib.rs".into(),
                at: Some(at),
            };
            panel.update(cx, |panel, cx| panel.view(place, window, cx));
        }))
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let viewed = panel.read(cx).viewing()?;
            let editor = viewed.editor.read(cx).editor()?;
            Some(editor.read(cx).selected_range())
        })
    };
    // "pub fn a() {}\npub fn b() {}\n": the second line starts at 14, its `b` is at 21.
    assert_eq!(go(At::Line(1), cx), Some(14..14));
    assert_eq!(go(At::Bytes(21..22), cx), Some(21..22));
    // Past the end, the end.
    assert_eq!(go(At::Line(9), cx), Some(28..28));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_is_coloured_by_its_name_its_extension_or_its_first_line() {
    let of = |path: &str, text: &str| language(Path::new(path), text);
    assert_eq!(of("crates/project/src/lib.rs", ""), "rust");
    assert_eq!(of("web/App.TSX", ""), "tsx");
    assert_eq!(of("Cargo.lock", ""), "toml");
    assert_eq!(of("Makefile", ""), "make");
    assert_eq!(of("script/check", "#!/bin/sh\nset -eu\n"), "bash");
    assert_eq!(of("script/run", "#!/usr/bin/env zsh\n"), "bash");
    assert_eq!(of("notes", "plain words\n"), "text");
    assert_eq!(of("data.unknown", ""), "text");
}

/// A checkout with a change and a file git doesn't know, and an ignored build folder.
fn checkout(name: &str) -> std::path::PathBuf {
    let root = scratch(name);
    git(&root, &["init", "-q", "-b", "main"]);
    write(&root, ".gitignore", b"target/\n");
    write(&root, "README.md", b"# Test\n");
    write(&root, "src/lib.rs", b"pub fn a() {}\n");
    write(&root, "src/main.rs", b"fn main() {}\n");
    write(&root, "src/nested/deep.rs", b"// deep\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    write(&root, "src/lib.rs", b"pub fn a() {}\npub fn b() {}\n");
    write(&root, "new.txt", b"new\n");
    write(&root, "target/debug/app", b"\0binary");
    root
}

/// The panel for the checkout at `root`, in a window of its own.
fn open(root: &Path, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<FilesPanel>) {
    // The project lists and watches the checkout on a thread of its own, which wakes the
    // panel's task from outside the test's scheduler.
    cx.executor().allow_parking();
    let root = root.to_owned();
    cx.update(gpui_kit::init);
    cx.update(settings::init);
    cx.update(right_panel::init);
    let (window, panel) = in_window(size(px(320.), px(600.)), cx, |window, cx| {
        let checkout = cx.new(|cx| Checkout::new(root, window, cx));
        cx.new(|cx| FilesPanel::new(checkout, cx))
    });
    settle(&panel, cx, |panel| panel.scanned);
    (window, panel)
}

/// Lets the panel take what the checkout's project publishes until it's `done`, waiting
/// for the project between looks.
fn settle(panel: &Entity<FilesPanel>, cx: &mut TestAppContext, done: impl Fn(&FilesPanel) -> bool) {
    // The checkout opens git, and with it the project the panel lists.
    cx.run_until_parked();
    let project = cx.read(|cx| panel.read(cx).checkout.read(cx).project().cloned());
    // Subscribed before looking, so whatever's published after the look wakes the wait.
    let updates = project
        .expect("the checkout opened")
        .subscribe(Feed::Everything);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| done(panel.read(cx))) {
            return;
        }
        let left = deadline.checked_duration_since(Instant::now());
        updates.next_within(left.expect("the panel never got there"));
    }
}

/// The rows shown, indented by depth, a folder's name ending in `/`.
fn shown(panel: &Entity<FilesPanel>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        (panel.read(cx).rows.rows().iter())
            .map(|row| {
                let slash = if row.entry.is_folder() { "/" } else { "" };
                format!(
                    "{}{}{slash}",
                    "  ".repeat(row.depth as usize),
                    row.entry.name
                )
            })
            .collect()
    })
}

fn selected(panel: &Entity<FilesPanel>, cx: &TestAppContext) -> Option<String> {
    cx.read(|cx| panel.read(cx).selected.as_ref().map(ToString::to_string))
}

#[gpui_kit::test]
fn the_tree_lists_the_checkout_and_marks_what_changed(cx: &mut TestAppContext) {
    let root = checkout("files-marks");
    let (_, panel) = open(&root, cx);
    assert_eq!(
        shown(&panel, cx),
        ["src/", "target/", ".gitignore", "new.txt", "README.md"]
    );
    cx.read(|cx| {
        let panel = panel.read(cx);
        let rows = panel.rows.rows();
        assert!(rows[1].entry.ignored && !rows[0].entry.ignored);
        let marks = panel.checkout.read(cx).marks().clone();
        assert_eq!(marks.folder("src"), Some(Mark::Modified));
        assert_eq!(
            marks.file("src/lib.rs").map(|file| file.mark),
            Some(Mark::Modified)
        );
        assert_eq!(
            marks.file("new.txt").map(|file| file.mark),
            Some(Mark::Untracked)
        );
    });
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn the_keyboard_walks_the_tree_and_shows_files(cx: &mut TestAppContext) {
    let root = checkout("files-keys");
    let (window, panel) = open(&root, cx);
    cx.update_window(window, |_, window, cx| {
        let focus = panel.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        window.draw(cx).clear(cx);
    })
    .unwrap();
    let press = |keys: &str, cx: &mut TestAppContext| {
        cx.simulate_keystrokes(window, keys);
        cx.run_until_parked();
        selected(&panel, cx)
    };
    assert_eq!(press("down", cx).as_deref(), Some("src"));
    assert_eq!(press("right", cx).as_deref(), Some("src"));
    assert_eq!(press("right", cx).as_deref(), Some("src/nested"));
    assert_eq!(press("down down", cx).as_deref(), Some("src/main.rs"));
    press("enter", cx);
    let viewed = cx.read(|cx| panel.read(cx).viewing().map(|viewed| viewed.path.clone()));
    assert_eq!(viewed, Some(root.join("src/main.rs")));
    assert_eq!(press("left", cx).as_deref(), Some("src"));
    assert_eq!(press("left", cx).as_deref(), Some("src"));
    assert_eq!(shown(&panel, cx).len(), 5, "src closed again");
    assert_eq!(press("end", cx).as_deref(), Some("README.md"));
    assert_eq!(press("home", cx).as_deref(), Some("src"));
    assert_eq!(press("pagedown", cx).as_deref(), Some("README.md"));
    // An ignored folder is read when it's opened, and what's in it is shown dimmed.
    // j and k pass the folders over, and stop where no file is left.
    assert_eq!(press("home j", cx).as_deref(), Some(".gitignore"));
    assert_eq!(press("k", cx).as_deref(), Some(".gitignore"));
    assert_eq!(press("home down", cx).as_deref(), Some("target"));
    press("right", cx);
    settle(&panel, cx, |panel| {
        panel.rows.position("target/debug").is_some()
    });
    assert_eq!(&shown(&panel, cx)[1..3], ["target/", "  debug/"]);
    cx.read(|cx| assert!(panel.read(cx).rows.rows()[2].entry.ignored));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn the_file_shown_is_revealed_and_picked(cx: &mut TestAppContext) {
    let root = checkout("files-reveal");
    let (window, panel) = open(&root, cx);
    (cx.update_window(window, |_, window, cx| {
        let path = Path::new("src/nested/deep.rs");
        panel.update(cx, |panel, cx| panel.view(Place::file(path), window, cx));
    }))
    .unwrap();
    assert_eq!(selected(&panel, cx).as_deref(), Some("src/nested/deep.rs"));
    assert_eq!(
        shown(&panel, cx)[..4],
        ["src/", "  nested/", "    deep.rs", "  lib.rs"]
    );
    let _ = std::fs::remove_dir_all(&root);
}

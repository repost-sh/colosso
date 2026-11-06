use std::path::Path;

use gpui_kit::{AppContext as _, TestAppContext};

use super::editor::language;
use super::panel::FilesEvent;
use super::*;
use crate::changes::{Mark, Touched};
use crate::checkout::{Content, read_at_most};
use crate::settings::{self, SaveScope};
use crate::test_support::{scratch, write};

pub(crate) mod support;
pub(crate) use support::*;

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
    // However many views show a file, it's read once: the views look at the document, which
    // whoever opened the file holds.
    let view = |path: &Path, cx: &mut TestAppContext| {
        let document = checkout.update(cx, |checkout, cx| checkout.document(path, cx));
        let view = (cx.update_window(window, |_, window, cx| {
            cx.new(|cx| FileEditor::new(&document, true, window, cx))
        }))
        .unwrap();
        (view, document)
    };
    let ((lib_editor, lib_document), (lib_again, again_document), (_, readme_document)) =
        (view(&lib, cx), view(&lib, cx), view(&readme, cx));
    assert_eq!(lib_document, again_document);
    assert_ne!(lib_document, readme_document);
    cx.read(|cx| assert_eq!(lib_again.read(cx).document(), Some(lib_document.clone())));
    // Read off the main thread, then shown by each.
    cx.run_until_parked();
    for editor in [&lib_editor, &lib_again] {
        let text = cx.read(|cx| editor.read(cx).text(cx));
        assert_eq!(text.as_deref(), Some("pub fn a() {}\npub fn b() {}\n"));
    }
    // The document goes with its last holder; the views only looked at it.
    drop((lib_document, again_document));
    cx.read(|cx| assert_eq!(lib_editor.read(cx).document(), None));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_file_viewed_again_is_shown_in_the_editor_it_had(cx: &mut TestAppContext) {
    let root = checkout("files-viewed");
    let (window, shell, files) = shell_showing(&root, cx);
    let view = |path: &str, cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.view_file(Place::file(path), window, cx);
            });
        }))
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            let file = shell.read(cx).viewing().unwrap();
            let file = file.read(cx);
            (
                file.path.clone(),
                file.editor().expect("a text file's editor").clone(),
            )
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
    assert_eq!(selected(&files, cx).as_deref(), Some("src/lib.rs"));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_place_puts_the_cursor_on_its_line_or_its_bytes(cx: &mut TestAppContext) {
    let root = checkout("files-place");
    let (window, shell, _) = shell_showing(&root, cx);
    let go = |at: At, cx: &mut TestAppContext| {
        let (_, editor, _) = editing(window, &shell, "src/lib.rs", at, cx);
        cx.read(|cx| editor.read(cx).selected_range())
    };
    // "pub fn a() {}\npub fn b() {}\n": the second line starts at 14, its `b` is at 21.
    assert_eq!(go(At::Line(1), cx), 14..14);
    assert_eq!(go(At::Bytes(21..22), cx), 21..22);
    // Past the end, the end.
    assert_eq!(go(At::Line(9), cx), 28..28);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn cmd_s_saves_the_file_shown_or_every_unsaved_one_as_set(cx: &mut TestAppContext) {
    let root = checkout("files-save-scope");
    let (window, shell, _) = shell_showing(&root, cx);
    let edit = |path: &str, text: &str, cx: &mut TestAppContext| {
        let (_, editor, _) = editing(window, &shell, path, At::Bytes(0..0), cx);
        typed(window, &editor, text, cx);
    };
    let save = |cx: &mut TestAppContext| {
        press(window, "cmd-s", cx);
        cx.run_until_parked();
    };
    let on_disk = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
    // As set at first, ⌘S writes the file shown, from wherever the keyboard is: lib.rs keeps
    // its unsaved edit.
    edit("src/lib.rs", "// lib\n", cx);
    edit("src/main.rs", "// main\n", cx);
    let shell_root = cx.read(|cx| shell.read(cx).focus.clone());
    (cx.update_window(window, |_, window, cx| window.focus(&shell_root, cx))).unwrap();
    save(cx);
    assert_eq!(on_disk("src/main.rs"), "// main\nfn main() {}\n");
    assert_eq!(on_disk("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n");
    // Set to every file, ⌘S writes lib.rs too, from the keyboard in main.rs.
    cx.update(|cx| settings::update(cx, |settings| settings.save_scope = SaveScope::All));
    edit("src/main.rs", "// again\n", cx);
    save(cx);
    assert_eq!(
        on_disk("src/lib.rs"),
        "// lib\npub fn a() {}\npub fn b() {}\n"
    );
    assert_eq!(on_disk("src/main.rs"), "// again\n// main\nfn main() {}\n");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn typing_marks_the_file_unsaved_until_cmd_s_writes_it(cx: &mut TestAppContext) {
    let root = checkout("files-save");
    let (window, shell, _) = shell_showing(&root, cx);
    let (_, editor, document) = editing(window, &shell, "src/lib.rs", At::Bytes(14..14), cx);
    let lib = root.join("src/lib.rs");
    typed(window, &editor, "// typed\n", cx);
    let dirty = |cx: &TestAppContext| cx.read(|cx| document.read(cx).is_dirty());
    assert!(dirty(cx), "typed into, the file has unsaved edits");
    let on_disk = || std::fs::read_to_string(&lib).unwrap();
    assert_eq!(on_disk(), "pub fn a() {}\npub fn b() {}\n");
    // Cmd+S, with the keyboard in the editor, writes the text shown.
    cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    cx.simulate_keystrokes(window, "cmd-s");
    cx.run_until_parked();
    assert!(!dirty(cx), "saved, the edits are the file's");
    assert_eq!(on_disk(), "pub fn a() {}\n// typed\npub fn b() {}\n");
    // Undone to the text saved, there's nothing unsaved; undone past it, there is.
    let undo = |cx: &mut TestAppContext| {
        cx.simulate_keystrokes(window, "cmd-z");
        cx.run_until_parked();
    };
    typed(window, &editor, "more", cx);
    assert!(dirty(cx));
    undo(cx);
    assert!(!dirty(cx), "back at the text saved");
    undo(cx);
    assert!(dirty(cx), "before the text saved");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_file_read_again_goes_into_its_editor_as_edits(cx: &mut TestAppContext) {
    let root = checkout("files-reread");
    let (window, shell, panel) = shell_showing(&root, cx);
    // The cursor on `b`, 21 bytes in.
    let (view, editor, document) = editing(window, &shell, "src/lib.rs", At::Bytes(21..21), cx);
    let touch = |cx: &mut TestAppContext| {
        let checkout = cx.read(|cx| panel.read(cx).checkout.clone());
        checkout.update(cx, |checkout, cx| {
            checkout.touched(vec![Touched::File("src/lib.rs".into())], cx);
        });
        cx.run_until_parked();
    };
    // Written by another program, the file's read again, and the lines that differ go into
    // the editor as edits: the cursor keeps its place, in the same editor.
    write(
        &root,
        "src/lib.rs",
        b"// first\npub fn a() {}\npub fn b() {}\n",
    );
    touch(cx);
    let text = |cx: &TestAppContext| cx.read(|cx| view.read(cx).text(cx).unwrap());
    assert_eq!(text(cx), "// first\npub fn a() {}\npub fn b() {}\n");
    cx.read(|cx| {
        assert_eq!(view.read(cx).editor(), Some(&editor));
        assert_eq!(editor.read(cx).selected_range(), 30..30);
        assert!(!document.read(cx).is_dirty(), "a read isn't an edit");
    });
    // Under unsaved edits it isn't read again: they'd be lost.
    typed(window, &editor, "x", cx);
    write(&root, "src/lib.rs", b"// second\n");
    touch(cx);
    assert_eq!(text(cx), "// first\npub fn a() {}\npub fn xb() {}\n");
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
    // What the tree asks the shell to open, as each file's path and whether it's kept.
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let heard = asked.clone();
    let _asking = cx.update(|cx| {
        cx.subscribe(&panel, move |_, event: &FilesEvent, _| {
            let FilesEvent::Open { place, kept } = event;
            heard
                .borrow_mut()
                .push((place.path.display().to_string(), *kept));
        })
    });
    let press = |keys: &str, cx: &mut TestAppContext| {
        cx.simulate_keystrokes(window, keys);
        cx.run_until_parked();
        selected(&panel, cx)
    };
    assert_eq!(press("down", cx).as_deref(), Some("src"));
    assert_eq!(press("right", cx).as_deref(), Some("src"));
    assert_eq!(press("right", cx).as_deref(), Some("src/nested"));
    // Each file moved over is previewed; Enter keeps the one picked.
    assert_eq!(press("down down", cx).as_deref(), Some("src/main.rs"));
    press("enter", cx);
    assert_eq!(
        asked.borrow()[..],
        [
            ("src/lib.rs".to_owned(), false),
            ("src/main.rs".to_owned(), false),
            ("src/main.rs".to_owned(), true),
        ]
    );
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
    let (window, shell, panel) = shell_showing(&root, cx);
    with_shell(window, &shell, cx, |shell, window, cx| {
        shell.view_file(Place::file(Path::new("src/nested/deep.rs")), window, cx);
    });
    cx.run_until_parked();
    assert_eq!(selected(&panel, cx).as_deref(), Some("src/nested/deep.rs"));
    assert_eq!(
        shown(&panel, cx)[..4],
        ["src/", "  nested/", "    deep.rs", "  lib.rs"]
    );
    let _ = std::fs::remove_dir_all(&root);
}

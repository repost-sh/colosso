//! Files open in the workspace's tabs: a preview gives way to the next until it's kept, a
//! file open already is shown where it is, the tree opens files and follows the tab shown,
//! and a tab whose file has unsaved edits asks before it closes.

use gpui_kit::{AppContext as _, TestAppContext, VisualTestContext, px};

use super::super::closing::SaveDialog;
use super::super::tests::{
    answer, checkout, dialog, editing, selected, shell_with_tree, tabs, typed, with, with_shell,
};
use super::super::{At, Place};
use crate::modal;
use crate::settings;
use crate::test_support::write;

#[gpui_kit::test]
fn a_preview_gives_way_to_the_next_until_its_kept(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-preview");
    let (window, shell, files) = shell_with_tree(&root, cx);
    let open = |path: &str, kept: bool, cx: &mut TestAppContext| {
        with_shell(window, &shell, cx, |shell, window, cx| {
            if kept {
                shell.view_file(Place::file(path), window, cx);
            } else {
                shell.preview_file(Place::file(path), window, cx);
            }
        });
        cx.run_until_parked();
        tabs(&shell, cx)
    };
    assert_eq!(open("src/lib.rs", false, cx), ["[lib.rs*]"]);
    assert_eq!(open("README.md", false, cx), ["[README.md*]"]);
    assert_eq!(selected(&files, cx).as_deref(), Some("README.md"), "picked in the tree");
    // A file opened for good opens right after the tab shown; the preview stays.
    assert_eq!(open("src/main.rs", true, cx), ["README.md*", "[main.rs]"]);
    // The next preview takes the preview's tab, where it is.
    assert_eq!(open("new.txt", false, cx), ["[new.txt*]", "main.rs"]);
    // A file open already is shown in its tab.
    assert_eq!(open("src/main.rs", false, cx), ["new.txt*", "[main.rs]"]);
    // Kept, as a double click on its tab does, the preview stays when another's previewed.
    with_shell(window, &shell, cx, |shell, _, cx| {
        let (project, branch, _) = shell.workspace.shown_tab().unwrap();
        shell.keep_tab((project, branch, 0), cx);
    });
    assert_eq!(open("README.md", false, cx), ["new.txt", "main.rs", "[README.md*]"]);
    // Opening the preview's file for good keeps it.
    assert_eq!(open("README.md", true, cx), ["new.txt", "main.rs", "[README.md]"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_preview_typed_into_is_kept(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-preview-typed");
    let (window, shell, _) = shell_with_tree(&root, cx);
    let preview = |path: &str, cx: &mut TestAppContext| {
        with_shell(window, &shell, cx, |shell, window, cx| {
            shell.preview_file(Place::file(path), window, cx);
        });
        cx.run_until_parked();
    };
    preview("src/lib.rs", cx);
    let editor = cx.read(|cx| {
        let file = shell.read(cx).viewing().unwrap();
        file.read(cx).editor.read(cx).editor().cloned()
    });
    typed(window, &editor.unwrap(), "// typed\n", cx);
    assert_eq!(tabs(&shell, cx), ["[lib.rs+]"], "kept, and marked unsaved");
    // The next preview opens beside it.
    preview("README.md", cx);
    assert_eq!(tabs(&shell, cx), ["lib.rs+", "[README.md*]"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn the_tree_opens_files_in_tabs_and_follows_the_tab_shown(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-tree");
    let (window, shell, files) = shell_with_tree(&root, cx);
    (cx.update_window(window, |_, window, cx| {
        let focus = files.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        window.draw(cx).clear(cx);
    }))
    .unwrap();
    let press = |keys: &str, cx: &mut TestAppContext| {
        cx.simulate_keystrokes(window, keys);
        cx.run_until_parked();
        tabs(&shell, cx)
    };
    // Rows: src/, target/, .gitignore, new.txt, README.md. A folder opens nothing.
    assert!(press("down", cx).is_empty());
    assert_eq!(press("end", cx), ["[README.md*]"]);
    assert_eq!(press("up", cx), ["[new.txt*]"]);
    // Enter keeps it; the next file down is previewed after it. The tree keeps the keyboard.
    assert_eq!(press("enter", cx), ["[new.txt]"]);
    assert_eq!(press("down", cx), ["new.txt", "[README.md*]"]);
    let tree_focused = (cx.update_window(window, |_, window, cx| {
        files.read(cx).focus_handle().is_focused(window)
    }))
    .unwrap();
    assert!(tree_focused, "the tree keeps the keyboard");
    // The tab shown picks its file in the tree.
    with_shell(window, &shell, cx, |shell, _, cx| {
        let (project, branch, _) = shell.workspace.shown_tab().unwrap();
        shell.workspace.select_tab(project, branch, 0);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(selected(&files, cx).as_deref(), Some("new.txt"));
    // Previews turned off, the keys only move the pick, and a click opens a tab for good.
    cx.update(|cx| settings::update(cx, |settings| settings.preview_files = false));
    press("end", cx);
    assert_eq!(selected(&files, cx).as_deref(), Some("README.md"));
    assert_eq!(tabs(&shell, cx), ["[new.txt]", "README.md*"]);
    with(window, &files, cx, |files, _, cx| files.activate(4, false, cx));
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["new.txt", "[README.md]"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_tab_with_unsaved_edits_asks_before_it_closes(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-unsaved");
    let (window, shell, _) = shell_with_tree(&root, cx);
    let lib = root.join("src/lib.rs");
    // Opens the file and types `text` at its start, keeping no hold on its editor.
    let edit = |text: &str, cx: &mut TestAppContext| {
        let (_, editor, _) = editing(window, &shell, "src/lib.rs", At::Bytes(0..0), cx);
        typed(window, &editor, text, cx);
    };
    let close = |cx: &mut TestAppContext| {
        with_shell(window, &shell, cx, |shell, window, cx| {
            let at = shell.workspace.shown_tab().unwrap();
            shell.close_tab_at(at, window, cx);
        });
        cx.run_until_parked();
    };
    // Saved: the file's written, then the tab closes.
    edit("// kept\n", cx);
    close(cx);
    assert_eq!(tabs(&shell, cx), ["[lib.rs+]"], "still open, asked");
    answer(cx, SaveDialog::save);
    assert!(tabs(&shell, cx).is_empty(), "closed once saved");
    assert!(dialog::<SaveDialog>(cx).is_none(), "the question's answered");
    let on_disk = || std::fs::read_to_string(&lib).unwrap();
    assert_eq!(on_disk(), "// kept\npub fn a() {}\npub fn b() {}\n");
    // Not saved: the tab closes, the file as it was.
    edit("// lost\n", cx);
    close(cx);
    answer(cx, SaveDialog::close);
    assert!(tabs(&shell, cx).is_empty());
    assert_eq!(on_disk(), "// kept\npub fn a() {}\npub fn b() {}\n");
    // Clean, a tab closes without a word.
    editing(window, &shell, "src/lib.rs", At::Bytes(0..0), cx);
    close(cx);
    assert!(tabs(&shell, cx).is_empty());
    assert!(dialog::<SaveDialog>(cx).is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn the_question_fits_a_file_name_that_wraps_its_title(cx: &mut TestAppContext) {
    let root = checkout("files-tabs-long-name");
    let long = "src/attempt-log-check-newer.usecase.rs";
    write(&root, long, b"// long\n");
    let (window, shell, _) = shell_with_tree(&root, cx);
    // Types into the file at `path`, closes its tab, and says how tall the question asked
    // is and where its buttons end, before answering it.
    let asked_about = |path: &str, cx: &mut TestAppContext| {
        let (_, editor, _) = editing(window, &shell, path, At::Bytes(0..0), cx);
        typed(window, &editor, "// edit\n", cx);
        with_shell(window, &shell, cx, |shell, window, cx| {
            let at = shell.workspace.shown_tab().unwrap();
            shell.close_tab_at(at, window, cx);
        });
        cx.run_until_parked();
        let (asked, question) = dialog::<SaveDialog>(cx).expect("asked");
        let cx = &mut VisualTestContext::from_window(asked, cx);
        cx.run_until_parked();
        let buttons = cx.debug_bounds("dialog-footer").expect("the buttons");
        let height = cx.update(|window, _| window.bounds().size.height);
        cx.update(|window, cx| question.update(cx, |question, cx| question.close(window, cx)));
        (height, buttons.bottom())
    };
    let padding = px(modal::PADDING);
    let (short, buttons) = asked_about("src/lib.rs", cx);
    assert_eq!(buttons, short - padding, "the buttons end at the padding");
    let (tall, buttons) = asked_about(long, cx);
    assert!(tall > short, "a line more for the name");
    assert_eq!(buttons, tall - padding, "the buttons keep their padding");
    let _ = std::fs::remove_dir_all(&root);
}

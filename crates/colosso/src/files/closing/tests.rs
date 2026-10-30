//! ⌘W closes the tab shown, and with none to close the window; ⌘⇧W the window: each asks
//! about unsaved edits first, and the answer saves them or lets them go.

use gpui_kit::{AppContext as _, TestAppContext};

use super::SaveDialog;
use crate::files::tests::{answer, asked, checkout, editing, typed};
use crate::files::{At, Place};
use crate::modal;
use crate::test_support::{select, shell_with};

#[gpui_kit::test]
fn cmd_w_closes_the_tab_shown_then_the_window_asking_about_unsaved_edits_first(cx: &mut TestAppContext) {
    let root = checkout("files-closing-window");
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    // A first file shown through the shell makes the Files panel.
    (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.view_file(Place::file("README.md"), window, cx));
    }))
    .unwrap();
    cx.run_until_parked();
    let files = cx.read(|cx| shell.read(cx).right_panel.files().cloned()).expect("the Files panel");
    // Shows the file at `path` and types `text` at its start.
    let edit = |path: &str, text: &str, cx: &mut TestAppContext| {
        let (_, editor, _) = editing(window, &files, path, At::Bytes(0..0), cx);
        typed(window, &editor, text, cx);
    };
    // Presses `keys` with the keyboard where the last file shown left it.
    let press = |keys: &str, cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))).unwrap();
        cx.simulate_keystrokes(window, keys);
        cx.run_until_parked();
    };
    let tabs = |cx: &TestAppContext| cx.read(|cx| files.read(cx).tabs().len());
    let windows = |cx: &TestAppContext| cx.read(|cx| cx.windows().len());
    let title = |cx: &mut TestAppContext| -> Option<String> {
        let (_, dialog) = asked(cx)?;
        Some(cx.read(|cx| dialog.read(cx).title.to_string()))
    };
    let on_disk = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();

    // Clean, the tab shown closes without a word, and the window stays.
    press("cmd-w", cx);
    assert_eq!((tabs(cx), windows(cx)), (0, 1));
    // With unsaved edits, the tab's asked about; not saved, it closes and the file stays.
    edit("src/lib.rs", "// lib\n", cx);
    edit("src/main.rs", "// main\n", cx);
    press("cmd-w", cx);
    assert_eq!(title(cx).as_deref(), Some("Save the changes to main.rs?"));
    answer(cx, SaveDialog::close);
    assert_eq!((tabs(cx), windows(cx)), (1, 1));
    assert_eq!(on_disk("src/main.rs"), "fn main() {}\n");
    // ⌘⇧W asks about the unsaved file in the window; cancelled, the window stays.
    press("cmd-shift-w", cx);
    assert_eq!(title(cx).as_deref(), Some("Save the changes to lib.rs?"));
    answer(cx, |_, window, cx| modal::dismiss(window, cx));
    assert_eq!((tabs(cx), windows(cx)), (1, 1));
    // With two unsaved files it asks about both; saved, both are written and the window goes.
    edit("README.md", "# edited\n", cx);
    press("cmd-shift-w", cx);
    assert_eq!(title(cx).as_deref(), Some("Save the changes to 2 files?"));
    answer(cx, SaveDialog::save);
    assert_eq!(windows(cx), 0, "the window and the question are gone");
    assert_eq!(on_disk("src/lib.rs"), "// lib\npub fn a() {}\npub fn b() {}\n");
    assert_eq!(on_disk("README.md"), "# edited\n# Test\n");
    let _ = std::fs::remove_dir_all(&root);
}

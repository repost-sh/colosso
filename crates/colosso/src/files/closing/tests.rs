//! ⌘W closes the tab shown, and with none to close the window; ⌘⇧W the window: each asks
//! about unsaved edits first, and the answer saves them or lets them go.

use gpui_kit::TestAppContext;

use super::SaveDialog;
use crate::files::At;
use crate::files::tests::{answer, checkout, dialog, editing, press, shell_showing, typed};
use crate::modal;

#[gpui_kit::test]
fn cmd_w_closes_the_tab_shown_then_the_window_asking_about_unsaved_edits_first(cx: &mut TestAppContext) {
    let root = checkout("files-closing-window");
    let (window, _, files) = shell_showing(&root, cx);
    // Shows the file at `path` and types `text` at its start.
    let edit = |path: &str, text: &str, cx: &mut TestAppContext| {
        let (_, editor, _) = editing(window, &files, path, At::Bytes(0..0), cx);
        typed(window, &editor, text, cx);
    };
    let tabs = |cx: &TestAppContext| cx.read(|cx| files.read(cx).tabs().len());
    let windows = |cx: &TestAppContext| cx.read(|cx| cx.windows().len());
    let title = |cx: &mut TestAppContext| -> Option<String> {
        let (_, asked) = dialog::<SaveDialog>(cx)?;
        Some(cx.read(|cx| asked.read(cx).title.to_string()))
    };
    let on_disk = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();

    // Clean, the tab shown closes without a word, and the window stays.
    press(window, "cmd-w", cx);
    assert_eq!((tabs(cx), windows(cx)), (0, 1));
    // With unsaved edits, the tab's asked about; not saved, it closes and the file stays.
    edit("src/lib.rs", "// lib\n", cx);
    edit("src/main.rs", "// main\n", cx);
    press(window, "cmd-w", cx);
    assert_eq!(title(cx).as_deref(), Some("Save the changes to main.rs?"));
    answer(cx, SaveDialog::close);
    assert_eq!((tabs(cx), windows(cx)), (1, 1));
    assert_eq!(on_disk("src/main.rs"), "fn main() {}\n");
    // ⌘⇧W asks about the unsaved file in the window; cancelled, the window stays.
    press(window, "cmd-shift-w", cx);
    assert_eq!(title(cx).as_deref(), Some("Save the changes to lib.rs?"));
    answer(cx, |_, window, cx| modal::dismiss(window, cx));
    assert_eq!((tabs(cx), windows(cx)), (1, 1));
    // With two unsaved files it asks about both; saved, both are written and the window goes.
    edit("README.md", "# edited\n", cx);
    press(window, "cmd-shift-w", cx);
    assert_eq!(title(cx).as_deref(), Some("Save the changes to 2 files?"));
    answer(cx, SaveDialog::save);
    assert_eq!(windows(cx), 0, "the window and the question are gone");
    assert_eq!(on_disk("src/lib.rs"), "// lib\npub fn a() {}\npub fn b() {}\n");
    assert_eq!(on_disk("README.md"), "# edited\n# Test\n");
    let _ = std::fs::remove_dir_all(&root);
}

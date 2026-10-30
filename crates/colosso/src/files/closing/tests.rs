//! ⌘W closes what's looked at: the settings, the file shown, asking about unsaved edits
//! first, or else the tab in the middle; never the window. The window's red button asks
//! about every unsaved file, and the answer saves them or lets them go.

use gpui_kit::{AppContext as _, FocusHandle, TestAppContext, VisualTestContext};

use super::SaveDialog;
use crate::files::At;
use crate::files::tests::{answer, checkout, dialog, editing, press, shell_showing, typed};
use crate::modal;
use crate::navigation::Route;

#[gpui_kit::test]
fn cmd_w_closes_what_is_looked_at_never_the_window(cx: &mut TestAppContext) {
    let root = checkout("files-closing-tabs");
    let (window, shell, files) = shell_showing(&root, cx);
    let tabs = |cx: &TestAppContext| cx.read(|cx| files.read(cx).tabs().len());
    let middle = |cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.branch()).map_or(0, |branch| branch.tabs.len()))
    };
    let focus = |handle: FocusHandle, cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| window.focus(&handle, cx))).unwrap();
    };
    let tree = cx.read(|cx| files.read(cx).focus_handle().clone());
    let shell_root = cx.read(|cx| shell.read(cx).focus.clone());

    // A chat's open in the middle; with the keyboard in the file shown, ⌘W closes the file.
    press(window, "cmd-t", cx);
    assert_eq!(middle(cx), 1);
    press(window, "cmd-w", cx);
    assert_eq!((tabs(cx), middle(cx)), (0, 1));
    // With the keyboard in the tree and no file shown, nothing closes; on the shell, the chat.
    focus(tree, cx);
    press(window, "cmd-w", cx);
    assert_eq!(middle(cx), 1);
    focus(shell_root.clone(), cx);
    press(window, "cmd-w", cx);
    assert_eq!(middle(cx), 0);
    press(window, "cmd-w", cx);
    // A file shown is closed from wherever the keyboard is, asked about if it's unsaved.
    let (_, editor, _) = editing(window, &files, "src/lib.rs", At::Bytes(0..0), cx);
    typed(window, &editor, "// lib\n", cx);
    focus(shell_root, cx);
    press(window, "cmd-w", cx);
    let asked = dialog::<SaveDialog>(cx).map(|(_, asked)| cx.read(|cx| asked.read(cx).title.to_string()));
    assert_eq!(asked.as_deref(), Some("Save the changes to lib.rs?"));
    answer(cx, SaveDialog::close);
    assert_eq!(tabs(cx), 0);
    // On a settings page, ⌘W closes the settings.
    press(window, "cmd-,", cx);
    assert!(matches!(cx.read(|cx| shell.read(cx).route(cx)), Route::Settings(_)));
    press(window, "cmd-w", cx);
    assert_eq!(cx.read(|cx| shell.read(cx).route(cx)), Route::Workspace);
    assert_eq!(cx.read(|cx| cx.windows().len()), 1, "never the window");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn the_red_button_asks_about_every_unsaved_file(cx: &mut TestAppContext) {
    let root = checkout("files-closing-window");
    let (window, _, files) = shell_showing(&root, cx);
    for (path, text) in [("src/lib.rs", "// lib\n"), ("README.md", "# edited\n")] {
        let (_, editor, _) = editing(window, &files, path, At::Bytes(0..0), cx);
        typed(window, &editor, text, cx);
    }
    let title = |cx: &mut TestAppContext| -> Option<String> {
        let (_, asked) = dialog::<SaveDialog>(cx)?;
        Some(cx.read(|cx| asked.read(cx).title.to_string()))
    };
    let windows = |cx: &TestAppContext| cx.read(|cx| cx.windows().len());
    let on_disk = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
    let close = |cx: &mut TestAppContext| {
        let closed = VisualTestContext::from_window(window, cx).simulate_close();
        cx.run_until_parked();
        closed
    };

    // The red button asks about both files; cancelled, the window stays as it was.
    assert!(!close(cx));
    assert_eq!(title(cx).as_deref(), Some("Save the changes to 2 files?"));
    answer(cx, |_, window, cx| modal::dismiss(window, cx));
    assert_eq!((windows(cx), cx.read(|cx| files.read(cx).tabs().len())), (1, 2));
    assert_eq!(on_disk("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n");
    // Asked again and saved, both are written and the window goes with the question.
    assert!(!close(cx));
    answer(cx, SaveDialog::save);
    assert_eq!(windows(cx), 0);
    assert_eq!(on_disk("src/lib.rs"), "// lib\npub fn a() {}\npub fn b() {}\n");
    assert_eq!(on_disk("README.md"), "# edited\n# Test\n");
    let _ = std::fs::remove_dir_all(&root);
}

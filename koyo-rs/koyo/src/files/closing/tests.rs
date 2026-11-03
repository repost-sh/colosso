//! ⌘W closes what's looked at: the settings, or else the tab shown, asking about its files'
//! unsaved edits first; never the window. A pane of a split closed alone is asked about
//! alone. The window's red button asks about every unsaved file, and the answer saves them
//! or lets them go.

use gpui_kit::{AppContext as _, FocusHandle, TestAppContext, VisualTestContext};

use super::SaveDialog;
use crate::files::At;
use crate::files::tests::{
    answer, checkout, dialog, editing, press, shell_showing, tabs, typed, with_shell,
};
use crate::modal;
use crate::navigation::Route;

#[gpui_kit::test]
fn cmd_w_closes_what_is_looked_at_never_the_window(cx: &mut TestAppContext) {
    let root = checkout("files-closing-tabs");
    let (window, shell, files) = shell_showing(&root, cx);
    let focus = |handle: FocusHandle, cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| window.focus(&handle, cx))).unwrap();
    };
    let tree = cx.read(|cx| files.read(cx).focus_handle().clone());
    let shell_root = cx.read(|cx| shell.read(cx).focus.clone());

    // README.md's tab, and a chat after it: ⌘W closes the chat, shown, and then README.md.
    press(window, "cmd-t", cx);
    assert_eq!(tabs(&shell, cx), ["README.md", "[New chat]"]);
    press(window, "cmd-w", cx);
    assert_eq!(tabs(&shell, cx), ["[README.md]"]);
    // From the tree too: it closes the tab shown.
    focus(tree, cx);
    press(window, "cmd-w", cx);
    assert!(tabs(&shell, cx).is_empty());
    // A file with unsaved edits is asked about first, from wherever the keyboard is.
    let (_, editor, _) = editing(window, &shell, "src/lib.rs", At::Bytes(0..0), cx);
    typed(window, &editor, "// lib\n", cx);
    focus(shell_root, cx);
    press(window, "cmd-w", cx);
    let asked = dialog::<SaveDialog>(cx).map(|(_, asked)| cx.read(|cx| asked.read(cx).title.to_string()));
    assert_eq!(asked.as_deref(), Some("Save the changes to lib.rs?"));
    assert_eq!(tabs(&shell, cx), ["[lib.rs+]"], "open until answered");
    answer(cx, SaveDialog::close);
    assert!(tabs(&shell, cx).is_empty());
    // On a settings page, ⌘W closes the settings.
    press(window, "cmd-,", cx);
    assert!(matches!(cx.read(|cx| shell.read(cx).route(cx)), Route::Settings(_)));
    press(window, "cmd-w", cx);
    assert_eq!(cx.read(|cx| shell.read(cx).route(cx)), Route::Workspace);
    assert_eq!(cx.read(|cx| cx.windows().len()), 1, "never the window");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_pane_of_a_split_with_unsaved_edits_is_asked_about_alone(cx: &mut TestAppContext) {
    let root = checkout("files-closing-pane");
    let (window, shell, _) = shell_showing(&root, cx);
    let (_, editor, _) = editing(window, &shell, "src/lib.rs", At::Bytes(0..0), cx);
    typed(window, &editor, "// lib\n", cx);
    // lib.rs joins README.md side by side.
    with_shell(window, &shell, cx, |shell, _, cx| {
        let unit = shell.workspace.branch_mut().unwrap();
        assert!(unit.join_tabs(1, 0, true));
        cx.notify();
    });
    assert_eq!(tabs(&shell, cx), ["[README.md|lib.rs+]"]);
    let lib = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().unwrap();
        unit.tabs[0].panes()[1].id
    });
    with_shell(window, &shell, cx, |shell, window, cx| shell.close_pane(lib, window, cx));
    cx.run_until_parked();
    assert!(dialog::<SaveDialog>(cx).is_some(), "asked about lib.rs");
    // Kept: nothing closes.
    answer(cx, |_, window, cx| modal::dismiss(window, cx));
    assert_eq!(tabs(&shell, cx), ["[README.md|lib.rs+]"]);
    // Saved: written, and only its pane goes.
    with_shell(window, &shell, cx, |shell, window, cx| shell.close_pane(lib, window, cx));
    cx.run_until_parked();
    answer(cx, SaveDialog::save);
    assert_eq!(tabs(&shell, cx), ["[README.md]"]);
    let on_disk = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert_eq!(on_disk, "// lib\npub fn a() {}\npub fn b() {}\n");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_file_tab_closed_comes_back_with_cmd_shift_t_as_it_is_on_disk(cx: &mut TestAppContext) {
    let root = checkout("files-closing-reopen");
    let (window, shell, _) = shell_showing(&root, cx);
    let (_, editor, _) = editing(window, &shell, "src/lib.rs", At::Bytes(0..0), cx);
    typed(window, &editor, "// lost\n", cx);
    press(window, "cmd-w", cx);
    answer(cx, SaveDialog::close);
    assert_eq!(tabs(&shell, cx), ["[README.md]"]);
    press(window, "cmd-shift-t", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), ["README.md", "[lib.rs]"], "back where it was, kept");
    let text = cx.read(|cx| {
        let file = shell.read(cx).viewing().unwrap();
        file.read(cx).editor.read(cx).text(cx)
    });
    assert_eq!(text.as_deref(), Some("pub fn a() {}\npub fn b() {}\n"), "its edits let go");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn the_red_button_asks_about_every_unsaved_file(cx: &mut TestAppContext) {
    let root = checkout("files-closing-window");
    let (window, shell, _) = shell_showing(&root, cx);
    for (path, text) in [("src/lib.rs", "// lib\n"), ("README.md", "# edited\n")] {
        let (_, editor, _) = editing(window, &shell, path, At::Bytes(0..0), cx);
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
    assert_eq!((windows(cx), tabs(&shell, cx).len()), (1, 2));
    assert_eq!(on_disk("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n");
    // Asked again and saved, both are written and the window goes with the question.
    assert!(!close(cx));
    answer(cx, SaveDialog::save);
    assert_eq!(windows(cx), 0);
    assert_eq!(on_disk("src/lib.rs"), "// lib\npub fn a() {}\npub fn b() {}\n");
    assert_eq!(on_disk("README.md"), "# edited\n# Test\n");
    let _ = std::fs::remove_dir_all(&root);
}

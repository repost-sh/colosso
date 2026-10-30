//! A symbol renamed in place: F2 puts a field over it with its name selected; Escape
//! leaves it as it was; a new name typed over and Enter rename it in this file's editor
//! and in the other file, on disk.

use gpui_kit::{App, TestAppContext};
use serde_json::{Value, json};

use super::support::*;
use crate::language::State;
use crate::test_support::repo;

#[gpui_kit::test]
async fn f2_renames_the_symbol_here_and_in_the_other_file_and_escape_leaves_it(cx: &mut TestAppContext) {
    let root = repo("language-rename", &[("src/lib.rs", LIB), ("src/other.rs", OTHER)]);
    let (call, call_range) = find(LIB, "target");
    let (_, target_range) = find(OTHER, "target");
    let (launcher, _) = launcher(move |root, _| {
        let (lib, other) = (uri(&root.join("src/lib.rs")), uri(&root.join("src/other.rs")));
        let capabilities = json!({"renameProvider": {"prepareProvider": true}});
        let mut script = started_with(root, "src/lib.rs", capabilities);
        script.extend([
            // F2, then Escape: asked what's there, and nothing more.
            json!({"expect": {"method": "textDocument/prepareRename", "params": {
                "position": call_range["start"]}}}),
            json!({"reply": 1, "result": {"range": call_range, "placeholder": "target"}}),
            // F2 again, a new name typed over and Enter: the rename, and its edit here.
            json!({"expect": {"method": "textDocument/prepareRename"}}),
            json!({"reply": 2, "result": {"range": call_range, "placeholder": "target"}}),
            json!({"expect": {"method": "textDocument/rename", "params": {
                "position": call_range["start"], "newName": "aim"}}}),
            json!({"reply": 3, "result": {"changes": {
                (&lib): [{"range": call_range, "newText": "aim"}],
                other: [{"range": target_range, "newText": "aim"}]}}}),
            change(1, &call_range, "aim", &lib),
        ]);
        Value::Array(script)
    });
    let shell = shell(&[&root], launcher, cx);
    let lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let lib_editor = cx.read(|cx| editor(&lib, cx)).unwrap();
    let field = |cx: &App| lib.read(cx).rename_field().cloned();

    // F2 on the name: the field's over it, with the name selected, and has the keyboard.
    place_caret(shell.0, &lib_editor, call.start, cx);
    cx.simulate_keystrokes(shell.0, "f2");
    let watched = vec![changes(&server, cx), changes(&lib, cx)];
    until(cx, watched, |cx| field(cx).is_some()).await;
    let input = cx.read(field).unwrap();
    assert_eq!(cx.read(|cx| input.read(cx).value()).as_ref(), "target");
    assert_eq!(cx.read(|cx| input.read(cx).selected_range()), 0..6);
    draw(shell.0, cx);
    // Escape: the field goes, the name as it was, the keyboard back in the editor.
    cx.simulate_keystrokes(shell.0, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| field(cx).is_none()));
    assert!(editor_focused(shell.0, &lib_editor, cx));
    assert_eq!(cx.read(|cx| lib_editor.read(cx).value()).as_ref(), LIB);

    // F2 again, a new name typed over the old and Enter: renamed here, in the editor...
    cx.simulate_keystrokes(shell.0, "f2");
    let watched = vec![changes(&server, cx), changes(&lib, cx)];
    until(cx, watched, |cx| field(cx).is_some()).await;
    draw(shell.0, cx);
    cx.simulate_keystrokes(shell.0, "a i m enter");
    let renamed = LIB.replace("other::target()", "other::aim()");
    let watched = vec![changes(&server, cx), changes(&lib_editor, cx)];
    until(cx, watched, |cx| lib_editor.read(cx).value().as_ref() == renamed).await;
    assert!(cx.read(|cx| field(cx).is_none()));
    assert!(editor_focused(shell.0, &lib_editor, cx));
    // ...and in the other file, on disk, once the write's run.
    cx.run_until_parked();
    let other = std::fs::read_to_string(root.join("src/other.rs")).unwrap();
    assert_eq!(other, OTHER.replace("fn target()", "fn aim()"));
    let _ = std::fs::remove_dir_all(&root);
}

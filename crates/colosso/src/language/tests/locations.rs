//! The places a symbol leads to, listed under the editor: its several definitions to pick
//! from, and every reference to it, by file and by line, the lines from the server's text
//! for a file open on it and from the disk for another.

use gpui_kit::{App, AppContext as _, TestAppContext};
use serde_json::{Value, json};

use super::support::*;
use crate::files::At;
use crate::test_support::repo;

#[gpui_kit::test]
async fn several_definitions_are_listed_and_the_one_picked_opens_its_file(cx: &mut TestAppContext) {
    let root = repo(
        "language-definitions",
        &[("src/lib.rs", LIB), ("src/other.rs", OTHER)],
    );
    let (greet, greet_range) = find(LIB, "greet");
    let call = LIB.find("target()").unwrap() + 2;
    let (target, target_range) = find(OTHER, "target");
    let (launcher, _) = launcher(move |root, _| {
        let mut script = started(root, "src/lib.rs");
        script.extend([
            json!({"recv": "textDocument/definition"}),
            json!({"reply": 1, "result": [
                {"uri": uri(&root.join("src/lib.rs")), "range": greet_range},
                {"uri": uri(&root.join("src/other.rs")), "range": target_range}]}),
            json!({"expect": {"method": "textDocument/didOpen", "params": {"textDocument":
                {"uri": uri(&root.join("src/other.rs")), "text": OTHER}}}}),
        ]);
        Value::Array(script)
    });
    let (shell, _, server, lib_editor) = ready(&root, "src/lib.rs", launcher, cx).await;
    let files = cx.read(|cx| shell.1.read(cx).right_panel.files().cloned()).unwrap();
    let listed = |cx: &App| {
        let list = files.read(cx).locations()?.read(cx);
        Some((list.found().clone(), list.selected()))
    };

    // F12 on the call, with two definitions: they're listed under the file, this file's
    // line from the editor's text and the other's from the disk, the first picked.
    place_caret(shell.0, &lib_editor, call, cx);
    cx.simulate_keystrokes(shell.0, "f12");
    let watched = vec![changes(&server, cx), changes(&files, cx)];
    until(cx, watched, |cx| listed(cx).is_some()).await;
    let (found, selected) = cx.read(listed).unwrap();
    assert_eq!(found.title.as_ref(), "2 definitions of `target`");
    assert_eq!(selected, Some((0, 0)));
    let paths: Vec<&str> = found.files.iter().map(|file| file.relative.as_ref()).collect();
    assert_eq!(paths, ["src/lib.rs", "src/other.rs"]);
    let row = &found.files[0].rows[0];
    assert_eq!((row.line, row.text.as_ref()), (2, "pub fn greet() -> u32 {"));
    assert_eq!(&row.text[row.matched.clone()], "greet");
    assert_eq!(row.at, At::Bytes(greet));
    let row = &found.files[1].rows[0];
    assert_eq!(row.line, 1);
    assert_eq!(&row.text[row.matched.clone()], "target");
    assert_eq!(row.at, At::Bytes(target.clone()));

    // Down to the other file's, which it shows meanwhile, then Enter: the file's open, the
    // definition selected, and the editor has the keyboard.
    cx.simulate_keystrokes(shell.0, "down enter");
    let other = root.join("src/other.rs");
    let selected = |cx: &App| {
        let viewed = files.read(cx).viewing()?;
        let editor = editor(&viewed.editor, cx)?;
        (viewed.path == other).then(|| editor.read(cx).selected_range())
    };
    let watched = vec![changes(&server, cx), changes(&files, cx)];
    until(cx, watched, |cx| selected(cx).is_some()).await;
    assert_eq!(cx.read(selected), Some(target));
    let shown = cx.read(|cx| editor(&files.read(cx).viewing()?.editor, cx)).unwrap();
    assert!(editor_focused(shell.0, &shown, cx));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn references_are_listed_by_file_and_escape_closes_the_list(cx: &mut TestAppContext) {
    let root = repo(
        "language-references",
        &[("src/lib.rs", LIB), ("src/other.rs", OTHER)],
    );
    let (greet, greet_range) = find(LIB, "greet");
    let (call, call_range) = find(LIB, "target");
    let (target, target_range) = find(OTHER, "target");
    let (launcher, _) = launcher(move |root, _| {
        let mut script = started(root, "src/lib.rs");
        script.extend([
            json!({"expect": {"method": "textDocument/references", "params": {
                "position": call_range["start"], "context": {"includeDeclaration": true}}}}),
            // The server's order: the other file first, and this one's places out of order.
            json!({"reply": 1, "result": [
                {"uri": uri(&root.join("src/other.rs")), "range": target_range},
                {"uri": uri(&root.join("src/lib.rs")), "range": call_range},
                {"uri": uri(&root.join("src/lib.rs")), "range": greet_range}]}),
        ]);
        Value::Array(script)
    });
    let (shell, _, server, lib_editor) = ready(&root, "src/lib.rs", launcher, cx).await;
    let files = cx.read(|cx| shell.1.read(cx).right_panel.files().cloned()).unwrap();
    let listed = |cx: &App| Some(files.read(cx).locations()?.read(cx).found().clone());

    place_caret(shell.0, &lib_editor, call.start, cx);
    cx.simulate_keystrokes(shell.0, "shift-f12");
    let watched = vec![changes(&server, cx), changes(&files, cx)];
    until(cx, watched, |cx| listed(cx).is_some()).await;
    let found = cx.read(listed).unwrap();
    assert_eq!(found.title.as_ref(), "3 references to `target`");
    let paths: Vec<&str> = found.files.iter().map(|file| file.relative.as_ref()).collect();
    assert_eq!(paths, ["src/other.rs", "src/lib.rs"]);
    let rows: Vec<(u32, &str, &str, At)> = (found.files.iter())
        .flat_map(|file| file.rows.iter())
        .map(|row| (row.line, row.text.as_ref(), &row.text[row.matched.clone()], row.at.clone()))
        .collect();
    assert_eq!(
        rows,
        [
            (1, "pub const S: &str = \"é🦀\"; pub fn target() -> u32 { 42 }", "target", At::Bytes(target)),
            (2, "pub fn greet() -> u32 {", "greet", At::Bytes(greet)),
            (4, "other::target()", "target", At::Bytes(call)),
        ]
    );
    // The list has the keyboard; Escape closes it and gives the keyboard back.
    let list_focused = |cx: &mut TestAppContext| {
        cx.update_window(shell.0, |_, window, cx| {
            window.draw(cx).clear(cx);
            let list = files.read(cx).locations()?;
            Some(list.read(cx).has_focus(window))
        })
        .unwrap()
    };
    assert_eq!(list_focused(cx), Some(true));
    cx.simulate_keystrokes(shell.0, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| listed(cx).is_none()));
    assert!(editor_focused(shell.0, &lib_editor, cx));
    let _ = std::fs::remove_dir_all(&root);
}

//! Go to Symbol: ⌘⇧O lists the file's symbols, nested ones after what they're in, narrowed
//! as a name's typed, and Enter selects the one picked; ⌘⌥O asks the checkout's servers
//! for a name once the typing pauses, and Enter opens the file the symbol's in at it.

use std::time::Duration;

use gpui_kit::{App, TestAppContext};
use serde_json::{Value, json};

use super::support::*;
use crate::modal;
use crate::symbols::Symbols;
use crate::test_support::repo;

#[gpui_kit::test]
async fn the_file_s_symbols_are_listed_narrowed_as_typed_and_the_one_picked_selected(cx: &mut TestAppContext) {
    let root = repo("language-document-symbols", &[("src/lib.rs", LIB), ("src/other.rs", OTHER)]);
    let (greet, greet_range) = find(LIB, "greet");
    let (_, other_range) = find(LIB, "other");
    let (_, s_range) = find(LIB, "s");
    let (launcher, _) = launcher(move |root, _| {
        let mut script = started_with(root, "src/lib.rs", json!({"documentSymbolProvider": true}));
        script.extend([
            json!({"expect": {"method": "textDocument/documentSymbol", "params": {
                "textDocument": {"uri": uri(&root.join("src/lib.rs"))}}}}),
            json!({"reply": 1, "result": [
                {"name": "other", "kind": 2, "range": other_range, "selectionRange": other_range},
                {"name": "greet", "kind": 12, "detail": "fn() -> u32", "range": greet_range,
                 "selectionRange": greet_range, "children": [
                    {"name": "s", "kind": 13, "range": s_range, "selectionRange": s_range}]}]}),
        ]);
        Value::Array(script)
    });
    let (shell, _, server, lib_editor) = ready(&root, "src/lib.rs", launcher, cx).await;
    place_caret(shell.0, &lib_editor, 0, cx);

    // The palette lists the symbols, the nested one after what it's in.
    cx.simulate_keystrokes(shell.0, "cmd-shift-o");
    let palette = cx.read(modal::current::<Symbols>).expect("the palette opened");
    let window = cx.read(modal::window).unwrap();
    let watched = vec![changes(&server, cx), changes(&palette, cx)];
    until(cx, watched, |cx| !palette.read(cx).rows().is_empty()).await;
    let rows = cx.read(|cx| palette.read(cx).rows());
    let names: Vec<(&str, Option<&str>)> = (rows.iter())
        .map(|(name, container, ..)| (name.as_str(), container.as_deref()))
        .collect();
    assert_eq!(names, [("other", None), ("greet", None), ("s", Some("greet"))]);

    // Typing narrows them, the characters typed marked; Enter selects the one picked.
    cx.simulate_keystrokes(window, "g r");
    assert_eq!(
        cx.read(|cx| palette.read(cx).rows()),
        [("greet".to_owned(), None, "gr".to_owned(), None)]
    );
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert!(cx.read(modal::current::<Symbols>).is_none());
    assert_eq!(cx.read(|cx| lib_editor.read(cx).selected_range()), greet);
    assert!(editor_focused(shell.0, &lib_editor, cx));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_symbol_of_the_checkout_is_found_by_name_and_its_file_opened_at_it(cx: &mut TestAppContext) {
    let root = repo("language-workspace-symbols", &[("src/lib.rs", LIB), ("src/other.rs", OTHER)]);
    let (target, target_range) = find(OTHER, "target");
    let (launcher, _) = launcher(move |root, _| {
        let mut script = started_with(root, "src/lib.rs", json!({"workspaceSymbolProvider": true}));
        script.extend([
            // Asked once the typing pauses, for what's typed by then.
            json!({"expect": {"method": "workspace/symbol", "params": {"query": "tar"}}}),
            json!({"reply": 1, "result": [{"name": "target", "kind": 12, "containerName": "other",
                "location": {"uri": uri(&root.join("src/other.rs")), "range": target_range}}]}),
            json!({"expect": {"method": "textDocument/didOpen", "params": {"textDocument":
                {"uri": uri(&root.join("src/other.rs"))}}}}),
        ]);
        Value::Array(script)
    });
    let (shell, _, server, lib_editor) = ready(&root, "src/lib.rs", launcher, cx).await;
    place_caret(shell.0, &lib_editor, 0, cx);

    cx.simulate_keystrokes(shell.0, "cmd-alt-o");
    let palette = cx.read(modal::current::<Symbols>).expect("the palette opened");
    let window = cx.read(modal::window).unwrap();
    cx.simulate_keystrokes(window, "t a r");
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    let watched = vec![changes(&server, cx), changes(&palette, cx)];
    until(cx, watched, |cx| !palette.read(cx).rows().is_empty()).await;
    assert_eq!(
        cx.read(|cx| palette.read(cx).rows()),
        [(
            "target".to_owned(),
            Some("other".to_owned()),
            "tar".to_owned(),
            Some("src/other.rs".to_owned())
        )]
    );

    // Enter: the file's opened with the name selected, its bytes found in the text shown.
    cx.simulate_keystrokes(window, "enter");
    let files = cx.read(|cx| shell.1.read(cx).right_panel.files().cloned()).unwrap();
    let other = root.join("src/other.rs");
    let selected = |cx: &App| {
        let viewed = files.read(cx).viewing()?;
        let editor = editor(&viewed.editor, cx)?;
        (viewed.path == other).then(|| editor.read(cx).selected_range())
    };
    let watched = vec![changes(&server, cx), changes(&files, cx)];
    until(cx, watched, |cx| selected(cx).is_some()).await;
    assert_eq!(cx.read(selected), Some(target));
    assert!(cx.read(modal::current::<Symbols>).is_none());
    let _ = std::fs::remove_dir_all(&root);
}

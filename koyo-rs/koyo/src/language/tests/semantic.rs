//! Semantic colouring: the server's tokens, asked for once the file's shown, come to the
//! editor as the theme's names at character columns; the setting takes them out and puts
//! them back; the server can ask for them again; and typing asks for them again.

use gpui_kit::{App, SharedString, TestAppContext};
use koyo_lsp::lsp_types::{Position, Range};
use serde_json::{Value, json};

use super::support::*;
use crate::settings;
use crate::test_support::repo;

#[gpui_kit::test]
async fn the_server_s_tokens_colour_the_text_and_follow_the_setting_a_refresh_and_typing(
    cx: &mut TestAppContext,
) {
    let root = repo(
        "language-semantic",
        &[("src/lib.rs", LIB), ("src/other.rs", OTHER)],
    );
    let (launcher, _) = launcher(move |root, _| {
        let uri = uri(&root.join("src/lib.rs"));
        let legend = json!({"tokenTypes": ["keyword", "function", "namespace"],
            "tokenModifiers": ["documentation"]});
        let provider = json!({"semanticTokensProvider": {"legend": legend, "full": true}});
        let mut script = started_with(root, "src/lib.rs", provider);
        let data = json!([0, 0, 3, 0, 0, 0, 8, 5, 2, 0, 2, 7, 5, 1, 0, 1, 24, 3, 0, 0]);
        script.extend([
            json!({"expect": {"method": "textDocument/semanticTokens/full", "params": {
                "textDocument": {"uri": uri}}}}),
            json!({"reply": 1, "result": {"data": data}}),
            json!({"expect": {"method": "textDocument/semanticTokens/full"}}),
            json!({"reply": 2, "result": {"data": data}}),
            json!({"send": {"id": "r1", "method": "workspace/semanticTokens/refresh"}}),
            json!({"expect": {"id": "r1", "result": null}}),
            json!({"expect": {"method": "textDocument/semanticTokens/full"}}),
            json!({"reply": 4, "result": {"data": [0, 0, 3, 0, 0]}}),
            change(1, &json!({"start": at(3, 4), "end": at(3, 4)}), "y", &uri),
            json!({"expect": {"method": "textDocument/semanticTokens/full"}}),
            json!({"reply": 5, "result": {"data": []}}),
        ]);
        Value::Array(script)
    });
    let (shell, _, server, editor) = ready(&root, "src/lib.rs", launcher, cx).await;
    let tokens = |cx: &App| editor.read(cx).lsp().semantic_tokens().to_vec();
    let provided = |cx: &App| editor.read(cx).lsp().semantic_tokens_provider.is_some();
    let token = |line, start, end, name: &str| {
        (
            Range::new(Position::new(line, start), Position::new(line, end)),
            SharedString::from(name.to_owned()),
        )
    };
    let coloured = [
        token(0, 0, 3, "keyword"),
        token(2, 7, 12, "function"),
        token(3, 23, 26, "keyword"),
    ];

    after_the_pause(shell.0, &server, &editor, cx, |cx| tokens(cx).len() == 3).await;
    assert_eq!(cx.read(tokens), coloured);

    cx.update(|cx| settings::update(cx, |settings| settings.semantic_colouring = false));
    cx.run_until_parked();
    assert!(!cx.read(provided));
    assert!(cx.read(tokens).is_empty());
    cx.update(|cx| settings::update(cx, |settings| settings.semantic_colouring = true));
    after_the_pause(shell.0, &server, &editor, cx, |cx| tokens(cx).len() == 1).await;
    assert_eq!(cx.read(tokens), [token(0, 0, 3, "keyword")]);

    place_caret(shell.0, &editor, find(LIB, "let").0.start, cx);
    cx.simulate_keystrokes(shell.0, "y");
    after_the_pause(shell.0, &server, &editor, cx, |cx| tokens(cx).is_empty()).await;
    let _ = std::fs::remove_dir_all(&root);
}

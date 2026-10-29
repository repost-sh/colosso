//! Completions and code actions against the scripted fake: the word typed asks the server
//! once and filters its list as it goes on, the chosen row goes in as a snippet; cmd-. lists
//! what could be done about the code, and the action chosen is filled in and applied.

use gpui_kit::{App, TestAppContext};
use serde_json::{Value, json};

use super::support::*;
use crate::language::*;
use crate::test_support::repo;

const MAIN: &str = "fn main() {\n    \n}\n";

/// What the servers in these tests can do.
fn capabilities() -> Value {
    json!({
        "completionProvider": {"resolveProvider": true, "triggerCharacters": ["."]},
        "codeActionProvider": {"resolveProvider": true},
    })
}

#[gpui_kit::test]
async fn a_word_typed_lists_the_server_s_completions_and_enter_puts_the_chosen_in(
    cx: &mut TestAppContext,
) {
    let root = repo("language-completions", &[("src/main.rs", MAIN)]);
    let (launcher, _) = launcher(move |root, _| {
        let main = uri(&root.join("src/main.rs"));
        let greet = json!({"label": "greet", "kind": 3, "preselect": true, "sortText": "0002",
            "insertTextFormat": 2,
            "textEdit": {"range": {"start": at(1, 4), "end": at(1, 5)},
                "newText": "greet(${1:name})$0"}});
        let mut filled = greet.clone();
        filled["documentation"] = json!({"kind": "markdown", "value": "Says hello."});
        let mut script = started_with(root, "src/main.rs", capabilities());
        script.extend([
            change(1, &json!({"start": at(1, 4), "end": at(1, 4)}), "g", &main),
            // Not a trigger character: asked as if invoked, at the caret after the "g".
            json!({"expect": {"method": "textDocument/completion", "params": {
                "position": at(1, 5), "context": {"triggerKind": 1}}}}),
            json!({"reply": 1, "result": {"isIncomplete": false, "items": [
                {"label": "other_thing", "kind": 3},
                {"label": "grab", "kind": 3, "sortText": "0001"},
                greet,
            ]}}),
            // The row selected is filled in.
            json!({"expect": {"method": "completionItem/resolve", "params": {"label": "greet"}}}),
            json!({"reply": 2, "result": filled}),
            // The "r" narrows the list here, without asking again.
            change(2, &json!({"start": at(1, 5), "end": at(1, 5)}), "r", &main),
            status("warning", Some("typed gr")),
            change(3, &json!({"start": at(1, 4), "end": at(1, 6)}), "greet(name)", &main),
            status("warning", Some("completed")),
        ]);
        Value::Array(script)
    });
    let shell = shell(&[&root], launcher, cx);
    let main = open(&shell, &root, "src/main.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let editor = cx.read(|cx| editor(&main, cx)).unwrap();
    // On the empty line, after its indentation.
    place_caret(shell.0, &editor, MAIN.find("    \n").unwrap() + 4, cx);

    // The key with the character it types, which the test platform doesn't infer.
    cx.simulate_keystrokes(shell.0, "g->g");
    assert_eq!(cx.read(|cx| editor.read(cx).value().to_string()), "fn main() {\n    g\n}\n");
    let labels = |cx: &App| {
        let menu = editor.read(cx).completion_menu_state();
        menu.open.then(|| {
            let labels: Vec<String> = menu.items.iter().map(|item| item.label.clone()).collect();
            (menu.query.clone(), labels)
        })
    };
    let watched = vec![changes(&server, cx), changes(&editor, cx)];
    until(cx, watched, |cx| labels(cx).is_some()).await;
    assert_eq!(
        cx.read(labels).unwrap(),
        ("g".to_owned(), vec!["greet".to_owned(), "grab".to_owned(), "other_thing".to_owned()]),
        "the preselected row first, then by sort text, then the fuzzy match"
    );
    // The server's documentation for the row lands in it.
    let documented = |cx: &App| {
        let menu = editor.read(cx).completion_menu_state();
        menu.items.first().and_then(|item| item.documentation.clone())
    };
    let watched = vec![changes(&server, cx), changes(&editor, cx)];
    until(cx, watched, |cx| documented(cx).is_some()).await;
    draw(shell.0, cx);

    cx.simulate_keystrokes(shell.0, "r->r");
    let health = |cx: &App| server.read(cx).health().cloned();
    let watched = vec![changes(&server, cx), changes(&editor, cx)];
    until(cx, watched, |cx| {
        health(cx).as_deref() == Some("typed gr") && labels(cx).is_some_and(|(q, _)| q == "gr")
    })
    .await;
    assert_eq!(
        cx.read(labels).unwrap().1,
        ["greet", "grab"],
        "narrowed here: other_thing has no r after its g"
    );
    assert!(cx.read(documented).is_some(), "the row stays filled in");
    draw(shell.0, cx);

    // Enter: the snippet goes in over the whole word, its first stop selected.
    cx.simulate_keystrokes(shell.0, "enter");
    let watched = vec![changes(&server, cx), changes(&editor, cx)];
    until(cx, watched, |cx| health(cx).as_deref() == Some("completed")).await;
    let completed = "fn main() {\n    greet(name)\n}\n";
    let (text, selected) = cx.read(|cx| {
        let editor = editor.read(cx);
        (editor.value().to_string(), editor.selected_range())
    });
    assert_eq!(text, completed);
    let name = completed.find("name").unwrap();
    assert_eq!(selected, name..name + 4);
    assert!(cx.read(|cx| !editor.read(cx).completion_menu_state().open));
    // Tab: the last stop, after the call, and the snippet is over.
    draw(shell.0, cx);
    cx.simulate_keystrokes(shell.0, "tab");
    let end = completed.find(")\n").unwrap() + 1;
    assert_eq!(cx.read(|cx| editor.read(cx).selected_range()), end..end);
    assert_eq!(cx.read(|cx| editor.read(cx).value().to_string()), completed);
    let _ = std::fs::remove_dir_all(&root);
}

const CALL: &str = "fn main() {\n    greet()\n}\n";
const OTHER_FN: &str = "pub fn greet() {}\n";

#[gpui_kit::test]
async fn code_actions_list_what_the_server_offers_and_the_chosen_one_edits_the_files(
    cx: &mut TestAppContext,
) {
    let root = repo(
        "language-code-actions",
        &[("src/main.rs", CALL), ("src/other.rs", OTHER_FN)],
    );
    let (call, call_range) = find(CALL, "greet");
    let (launcher, _) = launcher(move |root, _| {
        let main = uri(&root.join("src/main.rs"));
        let other = uri(&root.join("src/other.rs"));
        let diagnostic = json!({"range": call_range, "severity": 1,
            "message": "cannot find function `greet`"});
        let import = json!({"title": "Import `other::greet`", "kind": "quickfix",
            "isPreferred": true});
        let mut filled = import.clone();
        filled["edit"] = json!({"changes": {
            main.clone(): [{"range": {"start": at(0, 0), "end": at(0, 0)},
                "newText": "use other::greet;\n"}],
            other: [{"range": {"start": at(0, 0), "end": at(0, 0)}, "newText": "// used\n"}],
        }});
        let mut script = started_with(root, "src/main.rs", capabilities());
        script.extend([
            json!({"send": {"method": "textDocument/publishDiagnostics", "params": {
                "uri": main, "version": 0, "diagnostics": [diagnostic]}}}),
            // Asked for the caret, with the diagnostic there.
            json!({"expect": {"method": "textDocument/codeAction", "params": {
                "range": {"start": at(1, 5), "end": at(1, 5)},
                "context": {"diagnostics": [diagnostic]}}}}),
            json!({"reply": 1, "result": [
                {"title": "Run it", "command": "run"},
                {"title": "Run it as an action", "command": {"title": "Run", "command": "run"}},
                {"title": "Extract into function", "kind": "refactor.extract",
                    "edit": {"changes": {}}},
                import,
            ]}),
            // The one chosen came without its edit: filled in first.
            json!({"expect": {"method": "codeAction/resolve",
                "params": {"title": "Import `other::greet`"}}}),
            json!({"reply": 2, "result": filled}),
            change(1, &json!({"start": at(0, 0), "end": at(0, 0)}), "use other::greet;\n", &main),
            status("warning", Some("applied")),
        ]);
        Value::Array(script)
    });
    let shell = shell(&[&root], launcher, cx);
    let main = open(&shell, &root, "src/main.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let editor = cx.read(|cx| editor(&main, cx)).unwrap();
    let watched = vec![changes(&server, cx), changes(&main, cx)];
    until(cx, watched, |cx| {
        (editor.read(cx).diagnostics()).is_some_and(|diagnostics| diagnostics.iter().count() > 0)
    })
    .await;
    place_caret(shell.0, &editor, call.start + 1, cx);

    cx.simulate_keystrokes(shell.0, "cmd-.");
    let titles = |cx: &App| {
        let menu = editor.read(cx).code_action_menu_state();
        menu.open.then(|| {
            (menu.items.iter())
                .map(|item| item.action.title.clone())
                .collect::<Vec<_>>()
        })
    };
    let watched = vec![changes(&server, cx), changes(&editor, cx)];
    until(cx, watched, |cx| titles(cx).is_some()).await;
    assert_eq!(
        cx.read(titles).unwrap(),
        ["Import `other::greet`", "Extract into function"],
        "the preferred fix first; a command, bare or as an action, isn't listed"
    );
    draw(shell.0, cx);

    // Enter: this file is edited in the editor, the other on disk.
    cx.simulate_keystrokes(shell.0, "enter");
    let health = |cx: &App| server.read(cx).health().cloned();
    let watched = vec![changes(&server, cx), changes(&editor, cx)];
    until(cx, watched, |cx| health(cx).as_deref() == Some("applied")).await;
    assert_eq!(
        cx.read(|cx| editor.read(cx).value().to_string()),
        format!("use other::greet;\n{CALL}")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/other.rs")).unwrap(),
        format!("// used\n{OTHER_FN}")
    );
    assert!(cx.read(|cx| !editor.read(cx).code_action_menu_state().open));
    let _ = std::fs::remove_dir_all(&root);
}

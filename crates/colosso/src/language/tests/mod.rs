//! The language servers in the real shell, against crates/lsp's scripted fake
//! (`src/bin/fake-lsp.rs`): each start runs a script of what the server says and what it
//! must be sent, in order. Positions are UTF-16, on lines where that differs from both bytes
//! and characters.

mod completions;
mod inlay_hints;
mod locations;
mod rename;
mod semantic;
mod support;
mod symbols;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui_kit::component::input::EditorState;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, Modifiers, MouseMoveEvent, PlatformInput,
    TestAppContext,
};
use serde_json::{Value, json};

use super::*;
use crate::test_support::repo;
use support::*;

#[gpui_kit::test]
async fn a_rust_file_starts_its_server_which_gets_its_text_and_shows_diagnostics(
    cx: &mut TestAppContext,
) {
    let files = [
        ("README.md", "# hi\n"),
        ("src/lib.rs", LIB),
        ("src/other.rs", OTHER),
    ];
    let root = repo("language-start", &files);
    let (bytes, range) = find(LIB, "\"x\"");
    let (launcher, starts) = launcher(move |root, _| {
        let lib = uri(&root.join("src/lib.rs"));
        json!([
            {"recv": "initialize"},
            {"reply": 0, "result": {"capabilities": {"positionEncoding": "utf-16"}}},
            {"recv": "initialized"},
            {"expect": {"method": "textDocument/didOpen", "params": {"textDocument":
                {"uri": lib, "languageId": "rust", "version": 0, "text": LIB}}}},
            status("ok", None),
            {"send": {"method": "textDocument/publishDiagnostics", "params": {"uri": lib,
                "version": 0, "diagnostics": [{"range": range, "severity": 1,
                "message": "mismatched types"}]}}},
        ])
    });
    let shell = shell(&[&root], launcher, cx);
    // A file no server knows starts nothing.
    open(&shell, &root, "README.md", cx);
    assert!(server(&root, cx).is_none());
    // The first Rust file starts the checkout's server, which is sent the file's text.
    let lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).expect("a server for the checkout");
    assert_eq!(settled(&server, cx).await, State::Ready);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    // Its diagnostics land on the bytes it meant, past characters UTF-16 counts twice.
    let shown = |cx: &App| {
        let editor = editor(&lib, cx)?;
        let diagnostics = editor.read(cx).diagnostics()?;
        let shown = diagnostics
            .iter()
            .map(|entry| (entry.range.clone(), entry.message.clone()));
        Some(shown.collect::<Vec<_>>()).filter(|shown| !shown.is_empty())
    };
    let watched = vec![changes(&server, cx), changes(&lib, cx)];
    until(cx, watched, |cx| shown(cx).is_some()).await;
    let shown = cx.read(shown).unwrap();
    assert_eq!(shown, [(bytes, "mismatched types".into())]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_s_server_and_language_go_by_its_extension() {
    let of = |path: &str| Kind::of(Path::new(path));
    assert_eq!(of("src/lib.rs"), Some(Kind::Rust));
    for path in ["web/app.ts", "web/App.TSX", "lib/index.mjs", "old.jsx", "types.d.cts"] {
        assert_eq!(of(path), Some(Kind::TypeScript), "{path}");
    }
    assert_eq!(of("tool.py"), Some(Kind::Python));
    assert_eq!(of("stubs/tool.pyi"), Some(Kind::Python));
    assert_eq!(of("README.md"), None);
    assert_eq!(of("Makefile"), None);
    let id = |path: &str| language_id(Path::new(path));
    assert_eq!(id("a.ts"), "typescript");
    assert_eq!(id("a.tsx"), "typescriptreact");
    assert_eq!(id("a.js"), "javascript");
    assert_eq!(id("a.jsx"), "javascriptreact");
    assert_eq!(id("a.py"), "python");
    assert_eq!(id("a.txt"), "plaintext");
    assert_eq!(Kind::TypeScript.name(), "typescript");
    assert_eq!(Kind::Python.name(), "pyrefly");
}

#[gpui_kit::test]
async fn a_typescript_file_starts_its_server_which_is_ready_once_initialized(cx: &mut TestAppContext) {
    const APP: &str = "export const said: string = greet(\"you\");\n";
    let root = repo("language-typescript", &[("src/app.ts", APP), ("src/lib.rs", LIB)]);
    let (launcher, _) = launcher(move |root, _| {
        let app = uri(&root.join("src/app.ts"));
        json!([
            {"recv": "initialize"},
            {"reply": 0, "result": {"capabilities": {}}},
            {"recv": "initialized"},
            {"expect": {"method": "textDocument/didOpen", "params": {"textDocument":
                {"uri": app, "languageId": "typescript", "version": 0, "text": APP}}}},
        ])
    });
    let shell = shell(&[&root], launcher, cx);
    open(&shell, &root, "src/app.ts", cx);
    let server = server(&root, cx).expect("a server for the checkout");
    assert_eq!(cx.read(|cx| server.read(cx).kind()), Kind::TypeScript);
    // It never says it's done loading, as rust-analyzer does: initialized, it's ready.
    assert_eq!(settled(&server, cx).await, State::Ready);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn hover_asks_the_server_and_a_definition_opens_its_file(cx: &mut TestAppContext) {
    let root = repo(
        "language-definition",
        &[("src/lib.rs", LIB), ("src/other.rs", OTHER)],
    );
    let (greet, greet_range) = find(LIB, "greet");
    let call = LIB.find("target()").unwrap() + 2;
    let (target, target_range) = find(OTHER, "target");
    let (launcher, _) = launcher(move |root, _| {
        let mut script = started(root, "src/lib.rs");
        let hover = json!({"kind": "markdown", "value": "```rust\npub fn greet() -> u32\n```"});
        script.extend([
            json!({"recv": "textDocument/hover"}),
            json!({"reply": 1, "result": {"contents": hover, "range": greet_range}}),
            json!({"recv": "textDocument/definition"}),
            json!({"reply": 2, "result": [{"uri": uri(&root.join("src/other.rs")),
                "range": target_range}]}),
            json!({"expect": {"method": "textDocument/didOpen", "params": {"textDocument":
                {"uri": uri(&root.join("src/other.rs")), "text": OTHER}}}}),
            status("warning", Some("other.rs opened")),
        ]);
        Value::Array(script)
    });
    let shell = shell(&[&root], launcher, cx);
    let lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let lib_editor = cx.read(|cx| editor(&lib, cx)).unwrap();

    // Hover: the pointer rests on the name, and the server's text pops up over its range.
    rest_pointer(shell.0, &lib_editor, greet.start + 1, cx);
    let watched = vec![changes(&server, cx), changes(&lib_editor, cx)];
    until(cx, watched, |cx| {
        lib_editor.read(cx).hover_popover().is_some()
    })
    .await;
    let (contents, range) = cx.read(|cx| {
        let popover = lib_editor.read(cx).hover_popover().unwrap();
        (popover.hover.contents.clone(), popover.symbol_range.clone())
    });
    let colosso_lsp::lsp_types::HoverContents::Markup(contents) = contents else {
        panic!("markdown, as the server sent it");
    };
    assert!(contents.value.contains("pub fn greet() -> u32"));
    assert_eq!(range, greet);

    // F12 on the call: the other file is shown, the definition selected.
    place_caret(shell.0, &lib_editor, call, cx);
    cx.simulate_keystrokes(shell.0, "f12");
    let other = root.join("src/other.rs");
    let selected = |cx: &App| {
        let files = shell.1.read(cx).right_panel.files()?.read(cx);
        let viewed = files.viewing()?;
        let editor = editor(&viewed.editor, cx)?;
        (viewed.path == other).then(|| editor.read(cx).selected_range())
    };
    let watched = vec![changes(&server, cx), changes(&shell.1, cx)];
    until(cx, watched, |cx| selected(cx).is_some()).await;
    assert_eq!(cx.read(selected), Some(target));
    // It has the keyboard, so the selection shows, and keeps it as frames go by.
    for _ in 0..3 {
        draw(shell.0, cx);
        cx.executor().advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
    }
    let shown = cx.read(|cx| {
        let files = shell.1.read(cx).right_panel.files()?.read(cx);
        editor(&files.viewing()?.editor, cx)
    });
    assert!(
        editor_focused(shell.0, &shown.unwrap(), cx),
        "the definition's editor has the keyboard"
    );
    // And it was opened on the server, with its text.
    let watched = vec![changes(&server, cx)];
    until(cx, watched, |cx| server.read(cx).health().is_some()).await;
    let health = cx.read(|cx| server.read(cx).health().cloned());
    assert_eq!(health.as_deref(), Some("other.rs opened"));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_crashing_server_restarts_with_backoff_then_waits_for_a_retry(cx: &mut TestAppContext) {
    let root = repo("language-crash", &[("src/lib.rs", LIB)]);
    let (launcher, starts) = launcher(
        |_, start| json!([{"recv": "initialize"}, {"stderr": format!("panicked: start {start}")}, {"exit": 101}]),
    );
    let shell = shell(&[&root], launcher, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    let starts = |cx: &TestAppContext| {
        cx.run_until_parked();
        starts.load(Ordering::SeqCst)
    };
    // Each crash waits longer before the restart: one second, two, four.
    for (crash, wait) in [(0, 1000), (1, 2000), (2, 4000)] {
        let said = format!("panicked: start {crash}");
        assert_eq!(settled(&server, cx).await, State::Crashed(said.into()));
        assert!(cx.read(|cx| server.read(cx).is_restarting()));
        cx.executor().advance_clock(Duration::from_millis(wait - 1));
        assert_eq!(starts(cx), crash + 1, "not restarted before the wait");
        cx.executor().advance_clock(Duration::from_millis(1));
        assert_eq!(starts(cx), crash + 2, "restarted after it");
    }
    // A fourth crash within five minutes, and it's left for the user to retry.
    let crashed = State::Crashed("panicked: start 3".into());
    assert_eq!(settled(&server, cx).await, crashed);
    assert!(!cx.read(|cx| server.read(cx).is_restarting()));
    cx.executor().advance_clock(Duration::from_mins(10));
    assert_eq!(starts(cx), 4);
    // Starting it from its menu in the status bar retries it.
    cx.update(|cx| {
        let Languages(registry) = cx.global::<Languages>().clone();
        registry.update(cx, |registry, cx| registry.restart(Kind::Rust, &root, cx));
    });
    assert_eq!(starts(cx), 5);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_server_with_no_file_open_stops_after_ten_minutes_if_the_settings_ask(
    cx: &mut TestAppContext,
) {
    let root = repo("language-idle", &[("src/lib.rs", LIB)]);
    let (launcher, _) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&root], launcher, cx);
    // As in Zed and VS Code, it keeps running unless the settings ask.
    let lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    drop(lib);
    close_files(&shell.1, &root, cx);
    cx.executor().advance_clock(IDLE + Duration::from_secs(1));
    cx.run_until_parked();
    assert!(self::server(&root, cx).is_some(), "kept");
    cx.update(|cx| settings::update(cx, |settings| settings.stop_idle_language_servers = true));
    let lib = open(&shell, &root, "src/lib.rs", cx);
    assert_eq!(settled(&server, cx).await, State::Ready);
    let client = cx.read(|cx| server.read(cx).client().cloned()).unwrap();
    // Its last file let go of, it stays a while in case another opens.
    drop(lib);
    close_files(&shell.1, &root, cx);
    cx.executor()
        .advance_clock(IDLE.saturating_sub(Duration::from_secs(1)));
    cx.run_until_parked();
    assert!(self::server(&root, cx).is_some(), "still running");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(self::server(&root, cx).is_none(), "let go of");
    let exit = client.exited().await;
    assert!(!exit.crashed, "shut down as asked: {exit:?}");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn past_the_limit_the_least_recently_used_server_stops_an_idle_one_first(
    cx: &mut TestAppContext,
) {
    let roots: Vec<PathBuf> = ([
        "language-lru-a",
        "language-lru-b",
        "language-lru-c",
        "language-lru-d",
    ]
    .iter())
    .map(|name| repo(name, &[("src/lib.rs", LIB), ("src/other.rs", OTHER)]))
    .collect();
    let [a, b, c, d] = [&roots[0], &roots[1], &roots[2], &roots[3]];
    let (launcher, _) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[a, b, c, d], launcher, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.most_language_servers = Some(2)));
    let ready = async |root: &Path, cx: &mut TestAppContext| {
        let server = server(root, cx).unwrap();
        assert_eq!(settled(&server, cx).await, State::Ready);
        server
    };
    // A, then B; then A used again, and left with no file open.
    drop(open(&shell, a, "src/lib.rs", cx));
    ready(a, cx).await;
    let _b = open(&shell, b, "src/lib.rs", cx);
    let b_server = ready(b, cx).await;
    drop(open(&shell, a, "src/other.rs", cx));
    close_files(&shell.1, a, cx);
    // C starts: the idle A stops for it, though B was used less recently.
    let _c = open(&shell, c, "src/lib.rs", cx);
    ready(c, cx).await;
    assert!(server(a, cx).is_none());
    assert_eq!(
        cx.read(|cx| b_server.read(cx).state().clone()),
        State::Ready
    );
    // D starts with none idle: the least recently used, B, stops; its file stays shown.
    let _d = open(&shell, d, "src/lib.rs", cx);
    ready(d, cx).await;
    assert_eq!(
        cx.read(|cx| b_server.read(cx).state().clone()),
        State::Stopped
    );
    let live = |cx: &TestAppContext| {
        cx.read(|cx| {
            let Languages(registry) = cx.global::<Languages>();
            let servers = registry.read(cx).servers();
            servers
                .iter()
                .filter(|server| server.read(cx).is_live())
                .count()
        })
    };
    assert_eq!(live(cx), 2);
    // With no limit, as Zed and VS Code have it, A starts again and nothing stops for it.
    cx.update(|cx| settings::update(cx, |settings| settings.most_language_servers = None));
    let _a = open(&shell, a, "src/lib.rs", cx);
    ready(a, cx).await;
    assert_eq!(live(cx), 3);
    for root in &roots {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// Rests the pointer on the character at `at` in `editor`, long enough for it to ask.
fn rest_pointer(
    window: AnyWindowHandle,
    editor: &Entity<EditorState>,
    at: usize,
    cx: &mut TestAppContext,
) {
    let pointer = cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear(cx);
        let character = editor.read(cx).range_to_bounds(&(at..at + 1));
        character.map(|character| character.center())
    });
    let position = pointer.unwrap().expect("the character on screen");
    (cx.update_window(window, |_, window, cx| {
        let modifiers = Modifiers::default();
        let pointer = MouseMoveEvent {
            position,
            modifiers,
            pressed_button: None,
        };
        window.dispatch_event(PlatformInput::MouseMove(pointer), cx);
    }))
    .unwrap();
    // The editor waits a moment before it asks.
    cx.executor().advance_clock(Duration::from_millis(200));
}

#[gpui_kit::test]
async fn a_server_turned_off_for_its_project_stops_and_starts_again_once_turned_on(
    cx: &mut TestAppContext,
) {
    let root = repo("language-off", &[("src/lib.rs", LIB), ("src/other.rs", OTHER)]);
    let (launcher, starts) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&root], launcher, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let turn = |on: bool, cx: &mut TestAppContext| {
        let scope = settings::Scope::Project(root.clone());
        cx.update(|cx| settings::update(cx, |s| s.set_server("rust-analyzer", &scope, on)));
        cx.run_until_parked();
    };
    turn(false, cx);
    let off = |cx: &TestAppContext| {
        cx.read(|cx| {
            let Languages(registry) = cx.global::<Languages>();
            registry.read(cx).is_off(Kind::Rust, &root)
        })
    };
    assert!(off(cx));
    assert_eq!(cx.read(|cx| server.read(cx).state().clone()), State::Stopped);
    // A file opened while it's off doesn't start it, nor does restarting it.
    drop(_lib);
    close_files(&shell.1, &root, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    cx.update(|cx| {
        let Languages(registry) = cx.global::<Languages>().clone();
        registry.update(cx, |registry, cx| registry.restart(Kind::Rust, &root, cx));
    });
    cx.run_until_parked();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    // Turned on again, it starts, with the files that were opened.
    turn(true, cx);
    assert!(!off(cx));
    assert_eq!(settled(&server, cx).await, State::Ready);
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_server_the_user_stopped_waits_for_them_to_start_it(cx: &mut TestAppContext) {
    let root = repo("language-held", &[("src/lib.rs", LIB), ("src/other.rs", OTHER)]);
    let (launcher, starts) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&root], launcher, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let act = |stop: bool, cx: &mut TestAppContext| {
        cx.update(|cx| {
            let Languages(registry) = cx.global::<Languages>().clone();
            registry.update(cx, |registry, cx| {
                if stop {
                    registry.stop(Kind::Rust, &root, cx);
                } else {
                    registry.restart(Kind::Rust, &root, cx);
                }
            });
        });
        cx.run_until_parked();
    };
    act(true, cx);
    assert_eq!(cx.read(|cx| server.read(cx).state().clone()), State::Stopped);
    // Another file doesn't start it again behind the user's back.
    drop(_lib);
    close_files(&shell.1, &root, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    cx.run_until_parked();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    act(false, cx);
    assert_eq!(settled(&server, cx).await, State::Ready);
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn edits_reach_the_server_as_they_happen_and_a_save_is_told(cx: &mut TestAppContext) {
    let root = repo("language-edits", &[("src/lib.rs", LIB)]);
    // After the crab: on line 3, past 21 UTF-16 units, where the crab counts two.
    let after_crab = LIB.find("🦀").unwrap() + "🦀".len();
    let (launcher, _) = launcher(move |root, _| {
        let mut script = started(root, "src/lib.rs");
        let lib = uri(&root.join("src/lib.rs"));
        let at = json!({"start": at(3, 21), "end": at(3, 21)});
        script.extend([
            change(1, &at, "y", &lib),
            status("warning", Some("changed")),
            json!({"expect": {"method": "textDocument/didSave",
                "params": {"textDocument": {"uri": lib}}}}),
            status("warning", Some("saved")),
        ]);
        Value::Array(script)
    });
    let shell = shell(&[&root], launcher, cx);
    let lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let lib_editor = cx.read(|cx| editor(&lib, cx)).unwrap();
    (cx.update_window(shell.0, |_, window, cx| {
        lib_editor.update(cx, |editor, cx| {
            editor.set_selected_range(after_crab..after_crab, cx);
            editor.insert("y", window, cx);
        });
    }))
    .unwrap();
    let health = |cx: &App| server.read(cx).health().cloned();
    let watched = vec![changes(&server, cx)];
    until(cx, watched, |cx| health(cx).as_deref() == Some("changed")).await;
    // Saved, the server hears so once the file's written.
    lib.update(cx, |view, cx| view.save(cx).detach());
    let watched = vec![changes(&server, cx)];
    until(cx, watched, |cx| health(cx).as_deref() == Some("saved")).await;
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        LIB.replacen("🦀", "🦀y", 1)
    );
    let _ = std::fs::remove_dir_all(&root);
}

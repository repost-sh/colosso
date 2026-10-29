//! What the language tests share: the fake server run from a script, a checkout of its
//! own, the shell in a window over it, and waits that wake as the fake answers.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use colosso_lsp::Launch;
use futures::StreamExt as _;
use futures::stream::{self, LocalBoxStream};
use gpui_kit::component::input::EditorState;
use gpui_kit::{AnyWindowHandle, App, AppContext as _, Entity, TestAppContext};
use serde_json::{Value, json};

use crate::files::{FileEditor, Place};
use crate::language::server::Server;
use crate::language::*;
use crate::shell::Shell;
use crate::test_support::{select, shell_with};

pub(super) const LIB: &str = "pub mod other;\n\npub fn greet() -> u32 {\n    let s = \"héllo 🦀\"; let n: u32 = \"x\";\n    other::target()\n}\n";
pub(super) const OTHER: &str =
    "// defined here\npub const S: &str = \"é🦀\"; pub fn target() -> u32 { 42 }\n";
/// How long a wait goes without anything it watches changing before the test fails.
pub(super) const PATIENCE: Duration = Duration::from_secs(10);

/// The fake server, which `cargo test --workspace` builds for crates/lsp's own tests.
pub(super) fn fake() -> PathBuf {
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/fake-lsp");
    let built = fake.is_file();
    assert!(
        built,
        "build it first: cargo build -p colosso-lsp --bin fake-lsp"
    );
    fake
}

/// Runs the fake on each start with the script `script` writes for the checkout and the
/// number of starts before; and counts the starts.
pub(super) fn launcher(
    script: impl Fn(&Path, usize) -> Value + Send + Sync + 'static,
) -> (Launcher, Arc<AtomicUsize>) {
    let program = fake();
    let starts = Arc::new(AtomicUsize::new(0));
    let counted = starts.clone();
    let launcher: Launcher = Arc::new(move |_, root| {
        let start = counted.fetch_add(1, Ordering::SeqCst);
        Ok(Launch {
            name: "fake".to_owned(),
            program: program.clone(),
            args: vec![script(root, start).to_string()],
            env: Vec::new(),
            cwd: root.to_owned(),
            settings: json!({}),
            initialization_options: None,
            log: None,
        })
    });
    (launcher, starts)
}

/// The start every script begins with: initialized, then the file at `path` opened.
pub(super) fn started(root: &Path, path: &str) -> Vec<Value> {
    vec![
        json!({"recv": "initialize"}),
        json!({"reply": 0, "result": {"capabilities": {"positionEncoding": "utf-16"}}}),
        json!({"recv": "initialized"}),
        json!({"expect": {"method": "textDocument/didOpen",
            "params": {"textDocument": {"uri": uri(&root.join(path))}}}}),
        status("ok", None),
    ]
}

pub(super) fn status(health: &str, message: Option<&str>) -> Value {
    let params = json!({"health": health, "quiescent": true, "message": message});
    json!({"send": {"method": "experimental/serverStatus", "params": params}})
}

pub(super) fn uri(path: &Path) -> String {
    colosso_lsp::uri::from_path(path)
        .unwrap()
        .as_str()
        .to_owned()
}

/// Where `needle` is in `text`: its bytes, and the same as a UTF-16 range.
pub(super) fn find(text: &str, needle: &str) -> (Range<usize>, Value) {
    let start = text.find(needle).unwrap();
    let end = start + needle.len();
    let position = |offset: usize| {
        let before = &text[..offset];
        let line_start = before.rfind('\n').map_or(0, |at| at + 1);
        let character = before[line_start..].encode_utf16().count();
        json!({"line": before.matches('\n').count(), "character": character})
    };
    (
        start..end,
        json!({"start": position(start), "end": position(end)}),
    )
}

/// The shell in a window, each of `roots` a project in its sidebar, its servers run from
/// `launcher`.
pub(super) fn shell(
    roots: &[&Path],
    launcher: Launcher,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>) {
    let shell = shell_with(roots, cx);
    cx.update(|cx| {
        install(launcher, cx);
        bind_keys(cx);
    });
    shell
}

/// Selects the checkout at `root`, and shows its file at `path` in the Files panel.
pub(super) fn open(
    (window, shell): &(AnyWindowHandle, Entity<Shell>),
    root: &Path,
    path: &str,
    cx: &mut TestAppContext,
) -> Entity<FileEditor> {
    select(shell, root, cx);
    (cx.update_window(*window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.view_file(Place::file(path), window, cx);
        });
    }))
    .unwrap();
    cx.run_until_parked();
    shown(shell, cx).unwrap().1
}

/// The file the Files panel shows, and its view.
pub(super) fn shown(
    shell: &Entity<Shell>,
    cx: &TestAppContext,
) -> Option<(PathBuf, Entity<FileEditor>)> {
    cx.read(|cx| {
        let files = shell.read(cx).right_panel.files()?.read(cx);
        let viewed = files.viewing()?;
        Some((viewed.path.clone(), viewed.editor.clone()))
    })
}

/// Lets go of every file the checkout at `root` showed: it leaves the sidebar's selection,
/// and its Files panel with its views goes with it.
pub(super) fn close_files(shell: &Entity<Shell>, root: &Path, cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| shell.right_panel.forget_files(root, cx));
    cx.run_until_parked();
}

/// The server for the checkout at `root`, while there is one.
pub(super) fn server(root: &Path, cx: &TestAppContext) -> Option<Entity<Server>> {
    cx.read(|cx| {
        let Languages(registry) = cx.global::<Languages>();
        (registry.read(cx).servers().into_iter()).find(|server| server.read(cx).root() == root)
    })
}

pub(super) fn editor(view: &Entity<FileEditor>, cx: &App) -> Option<Entity<EditorState>> {
    view.read(cx).editor().cloned()
}

/// Something to wait on: the changes of an entity.
pub(super) fn changes<T: 'static>(
    entity: &Entity<T>,
    cx: &mut TestAppContext,
) -> LocalBoxStream<'static, ()> {
    cx.notifications(entity).boxed_local()
}

/// Waits until `done`, looking again each time one of `watched` changes: the fake's answers
/// come from outside the scheduler, which parks until they wake it.
pub(super) async fn until(
    cx: &mut TestAppContext,
    watched: Vec<LocalBoxStream<'static, ()>>,
    done: impl Fn(&App) -> bool,
) {
    let mut watched = stream::select_all(watched);
    while !cx.read(&done) {
        let changed = colosso_lsp::within(PATIENCE, watched.next()).await;
        let changed = changed.expect("nothing changed for too long, and it never got there");
        changed.expect("what was watched is gone");
    }
}

/// Waits until the server at `root` has settled: ready, or failed or crashed.
pub(super) async fn settled(server: &Entity<Server>, cx: &mut TestAppContext) -> State {
    let watched = vec![changes(server, cx)];
    let busy = |state: &State| matches!(state, State::Starting | State::Indexing { .. });
    until(cx, watched, |cx| !busy(server.read(cx).state())).await;
    cx.read(|cx| server.read(cx).state().clone())
}

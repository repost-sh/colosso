//! A checkout's servers put to sleep while it's out of sight ([`SLEEP`]) and woken once it's
//! seen: which start again and with what, which stay stopped, and how a closed window or one
//! out of sight from the start counts.

use std::path::Path;
use std::time::Duration;

use gpui_kit::{App, AppContext as _, Entity, SharedString, TestAppContext};
use serde_json::{Value, json};

use super::support::*;
use crate::language::*;
use crate::projects::read_worktrees;
use crate::shell::Shell;
use crate::test_support::{on_screen, repo, select, shell_window};

const SECOND: Duration = Duration::from_secs(1);
/// A second short of [`SLEEP`].
const ALMOST: Duration = SLEEP.saturating_sub(SECOND);

/// Lets `time` go by, and runs what it wakes.
fn pass(time: Duration, cx: &mut TestAppContext) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
}

/// Adds the checkout at `root` to the sidebar, a project of its own.
fn add(shell: &Entity<Shell>, root: &Path, cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| {
        shell.workspace.add_project(root.to_owned());
        shell.show_worktrees(root, read_worktrees(root), cx);
    });
}

/// What the status bar says beside each of the selected checkout's servers.
fn said(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<Option<SharedString>> {
    cx.read(|cx| shell.read(cx).status_bar.servers().read(cx).words(cx))
}

#[gpui_kit::test]
async fn a_checkout_unselected_for_ten_minutes_sleeps_and_wakes_with_its_files_once_selected(
    cx: &mut TestAppContext,
) {
    let a = repo("language-sleep-a", &[("src/lib.rs", LIB)]);
    let b = repo("language-sleep-b", &[("README.md", "hi\n")]);
    let (bytes, range) = find(LIB, "\"x\"");
    let (launcher, starts) = launcher(move |root, start| {
        let lib = uri(&root.join("src/lib.rs"));
        let mut script = vec![
            json!({"recv": "initialize"}),
            json!({"reply": 0, "result": {"capabilities": {"positionEncoding": "utf-16"}}}),
            json!({"recv": "initialized"}),
            json!({"expect": {"method": "textDocument/didOpen", "params": {"textDocument":
                {"uri": lib, "languageId": "rust", "version": 0, "text": LIB}}}}),
            status("ok", None),
        ];
        if start == 1 {
            script.push(json!({"send": {"method": "textDocument/publishDiagnostics",
                "params": {"uri": lib, "version": 0, "diagnostics": [{"range": range,
                "severity": 1, "message": "mismatched types"}]}}}));
        }
        Value::Array(script)
    });
    let shell = shell(&[&a, &b], launcher, cx);
    let lib = open(&shell, &a, "src/lib.rs", cx);
    let server = server(&a, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let client = cx.read(|cx| server.read(cx).client().cloned()).unwrap();
    select(&shell.1, &b, cx);
    pass(ALMOST, cx);
    assert_eq!(state(&server, cx), State::Ready, "running a second before");
    pass(SECOND, cx);
    assert_eq!(state(&server, cx), State::Stopped, "asleep at ten minutes");
    let exit = client.exited().await;
    assert!(!exit.crashed, "shut down as asked: {exit:?}");
    assert!(self::server(&a, cx) == Some(server.clone()), "kept");
    select(&shell.1, &a, cx);
    assert_eq!(
        settled(&server, cx).await,
        State::Ready,
        "given its file again"
    );
    assert_eq!(count(&starts, cx), 2);
    let shown = |cx: &App| {
        let diagnostics = editor(&lib, cx)?.read(cx).diagnostics()?;
        let shown = (diagnostics.iter()).map(|entry| (entry.range.clone(), entry.message.clone()));
        Some(shown.collect::<Vec<_>>()).filter(|shown| !shown.is_empty())
    };
    let watched = vec![changes(&server, cx), changes(&lib, cx)];
    until(cx, watched, |cx| shown(cx).is_some()).await;
    assert_eq!(
        cx.read(shown).unwrap(),
        [(bytes, "mismatched types".into())]
    );
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

#[gpui_kit::test]
async fn a_checkout_selected_again_within_ten_minutes_keeps_its_servers_running(
    cx: &mut TestAppContext,
) {
    let a = repo("language-awake-a", &[("src/lib.rs", LIB)]);
    let b = repo("language-awake-b", &[("README.md", "hi\n")]);
    let (launcher, starts) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&a, &b], launcher, cx);
    let _lib = open(&shell, &a, "src/lib.rs", cx);
    let server = server(&a, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    select(&shell.1, &b, cx);
    pass(ALMOST, cx);
    select(&shell.1, &a, cx);
    pass(ALMOST, cx);
    assert_eq!(state(&server, cx), State::Ready, "never put to sleep");
    assert_eq!(count(&starts, cx), 1);
    select(&shell.1, &b, cx);
    pass(ALMOST, cx);
    assert_eq!(
        state(&server, cx),
        State::Ready,
        "counting from the start again"
    );
    pass(SECOND, cx);
    assert_eq!(state(&server, cx), State::Stopped);
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

#[gpui_kit::test]
async fn a_checkout_in_a_window_out_of_sight_for_ten_minutes_sleeps_and_wakes_once_seen(
    cx: &mut TestAppContext,
) {
    let root = repo("language-sleep-hidden", &[("src/lib.rs", LIB)]);
    let (launcher, starts) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&root], launcher, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    on_screen(shell.0, false, cx);
    pass(ALMOST, cx);
    assert_eq!(state(&server, cx), State::Ready, "running a second before");
    pass(SECOND, cx);
    assert_eq!(state(&server, cx), State::Stopped, "asleep at ten minutes");
    assert_eq!(said(&shell.1, cx), [Some("asleep".into())]);
    on_screen(shell.0, true, cx);
    assert_eq!(
        settled(&server, cx).await,
        State::Ready,
        "given its file again"
    );
    assert_eq!(count(&starts, cx), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_server_the_user_stopped_stays_stopped_through_sleeping_and_waking(
    cx: &mut TestAppContext,
) {
    let root = repo("language-sleep-held", &[("src/lib.rs", LIB)]);
    let (launcher, starts) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&root], launcher, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    cx.update(|cx| {
        let Languages(registry) = cx.global::<Languages>().clone();
        registry.update(cx, |registry, cx| registry.stop(Kind::Rust, &root, cx));
    });
    on_screen(shell.0, false, cx);
    pass(SLEEP, cx);
    assert_eq!(said(&shell.1, cx), [Some("stopped".into())]);
    on_screen(shell.0, true, cx);
    assert_eq!(state(&server, cx), State::Stopped);
    assert_eq!(count(&starts, cx), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_checkout_leaving_the_sidebar_asleep_lets_go_of_its_servers(cx: &mut TestAppContext) {
    let a = repo("language-sleep-gone-a", &[("src/lib.rs", LIB)]);
    let b = repo("language-sleep-gone-b", &[("README.md", "hi\n")]);
    let (launcher, _) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&a, &b], launcher, cx);
    let _lib = open(&shell, &a, "src/lib.rs", cx);
    let server = server(&a, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    select(&shell.1, &b, cx);
    pass(SLEEP, cx);
    assert_eq!(state(&server, cx), State::Stopped, "asleep");
    shell.1.update(cx, |shell, cx| shell.remove_project(&a, cx));
    cx.run_until_parked();
    assert!(self::server(&a, cx).is_none(), "let go of");
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

#[gpui_kit::test]
async fn a_server_stopped_for_the_limit_stays_stopped_when_its_checkout_wakes(
    cx: &mut TestAppContext,
) {
    let a = repo("language-sleep-limit-a", &[("src/lib.rs", LIB)]);
    let b = repo("language-sleep-limit-b", &[("src/lib.rs", LIB)]);
    let (launcher, starts) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&a, &b], launcher, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.most_language_servers = Some(1)));
    let _a = open(&shell, &a, "src/lib.rs", cx);
    let a_server = server(&a, cx).unwrap();
    assert_eq!(settled(&a_server, cx).await, State::Ready);
    let _b = open(&shell, &b, "src/lib.rs", cx);
    assert_eq!(settled(&server(&b, cx).unwrap(), cx).await, State::Ready);
    assert_eq!(
        state(&a_server, cx),
        State::Stopped,
        "stopped for the limit"
    );
    pass(SLEEP, cx);
    select(&shell.1, &a, cx);
    assert_eq!(
        state(&a_server, cx),
        State::Stopped,
        "still stopped once seen"
    );
    assert_eq!(count(&starts, cx), 2);
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

#[gpui_kit::test]
async fn a_server_with_no_file_open_stays_stopped_when_its_checkout_wakes(cx: &mut TestAppContext) {
    let root = repo("language-sleep-idle", &[("src/lib.rs", LIB)]);
    let (launcher, starts) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&root], launcher, cx);
    drop(open(&shell, &root, "src/lib.rs", cx));
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    close_files(&shell.1, &root, cx);
    on_screen(shell.0, false, cx);
    pass(SLEEP, cx);
    assert_eq!(state(&server, cx), State::Stopped, "asleep");
    on_screen(shell.0, true, cx);
    assert_eq!(state(&server, cx), State::Stopped, "no file to start for");
    assert_eq!(count(&starts, cx), 1);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    assert_eq!(
        settled(&server, cx).await,
        State::Ready,
        "started for a file"
    );
    assert_eq!(count(&starts, cx), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_server_made_while_asleep_keeps_its_file_and_starts_with_it_at_wake(
    cx: &mut TestAppContext,
) {
    let root = repo("language-sleep-made", &[("src/lib.rs", LIB)]);
    let (launcher, starts) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&root], launcher, cx);
    select(&shell.1, &root, cx);
    on_screen(shell.0, false, cx);
    pass(SLEEP, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(
        state(&server, cx),
        State::Stopped,
        "not started while asleep"
    );
    assert_eq!(count(&starts, cx), 0);
    on_screen(shell.0, true, cx);
    assert_eq!(settled(&server, cx).await, State::Ready, "given its file");
    assert_eq!(count(&starts, cx), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_server_turned_on_while_asleep_starts_only_once_its_checkout_wakes(
    cx: &mut TestAppContext,
) {
    let root = repo("language-sleep-turned-on", &[("src/lib.rs", LIB)]);
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
    on_screen(shell.0, false, cx);
    pass(SLEEP, cx);
    turn(true, cx);
    assert_eq!(
        state(&server, cx),
        State::Stopped,
        "not started while asleep"
    );
    assert_eq!(count(&starts, cx), 1);
    on_screen(shell.0, true, cx);
    assert_eq!(settled(&server, cx).await, State::Ready, "started at wake");
    assert_eq!(count(&starts, cx), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_checkout_whose_window_closed_sleeps_and_wakes_in_the_window_opened_again(
    cx: &mut TestAppContext,
) {
    let root = repo("language-sleep-closed", &[("src/lib.rs", LIB)]);
    let (launcher, _) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&root], launcher, cx);
    drop(open(&shell, &root, "src/lib.rs", cx));
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    let (window, shell) = shell;
    drop(shell);
    (cx.update_window(window, |_, window, _| window.remove_window())).unwrap();
    pass(ALMOST, cx);
    assert_eq!(state(&server, cx), State::Ready, "running a second before");
    pass(SECOND, cx);
    assert_eq!(state(&server, cx), State::Stopped, "asleep at ten minutes");
    let (window, shell) = shell_window(cx);
    add(&shell, &root, cx);
    let _lib = open(&(window, shell), &root, "src/lib.rs", cx);
    assert_eq!(settled(&server, cx).await, State::Ready, "woken");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
async fn a_checkout_first_selected_in_a_window_out_of_sight_sleeps_after_ten_minutes(
    cx: &mut TestAppContext,
) {
    let a = repo("language-sleep-unseen-a", &[("README.md", "hi\n")]);
    let b = repo("language-sleep-unseen-b", &[("src/lib.rs", LIB)]);
    let (launcher, _) = launcher(|root, _| Value::Array(started(root, "src/lib.rs")));
    let shell = shell(&[&a], launcher, cx);
    select(&shell.1, &a, cx);
    on_screen(shell.0, false, cx);
    add(&shell.1, &b, cx);
    let _lib = open(&shell, &b, "src/lib.rs", cx);
    let server = server(&b, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    pass(ALMOST, cx);
    assert_eq!(state(&server, cx), State::Ready, "running a second before");
    pass(SECOND, cx);
    assert_eq!(state(&server, cx), State::Stopped, "asleep at ten minutes");
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

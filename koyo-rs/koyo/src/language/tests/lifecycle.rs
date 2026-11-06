//! When a checkout's server runs and when it stops: a crash restarts it after a longer wait each
//! time, until it's left for the user to retry; it stops once idle if the settings ask, for
//! another past the limit, while turned off for its project, and when the user stops it.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui_kit::TestAppContext;
use serde_json::{Value, json};

use super::support::*;
use crate::language::*;
use crate::test_support::repo;

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

#[gpui_kit::test]
async fn a_server_turned_off_for_its_project_stops_and_starts_again_once_turned_on(
    cx: &mut TestAppContext,
) {
    let root = repo(
        "language-off",
        &[("src/lib.rs", LIB), ("src/other.rs", OTHER)],
    );
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
    assert_eq!(
        cx.read(|cx| server.read(cx).state().clone()),
        State::Stopped
    );
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
    let root = repo(
        "language-held",
        &[("src/lib.rs", LIB), ("src/other.rs", OTHER)],
    );
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
    assert_eq!(
        cx.read(|cx| server.read(cx).state().clone()),
        State::Stopped
    );
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

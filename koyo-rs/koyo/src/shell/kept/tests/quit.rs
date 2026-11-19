//! The window kept as the app quits: a change still settling, a window closed just before,
//! and a write stuck on a disk that stalled after the window was opened again.

use std::time::Instant;

use gpui_kit::{AppContext as _, TestAppContext};

use super::{file, launch, main_with, page, read_file};
use crate::kept_file::{Stuck, WAITS};
use crate::shell::kept::Kept;
use crate::test_support::{open_page, repo, select, shell_window, shell_with};

#[gpui_kit::test]
fn a_change_still_settling_is_written_when_the_app_quits(cx: &mut TestAppContext) {
    let root = repo("kept-window-quit", &[("README.md", "hi\n")]);
    let path = file("kept-window-quit-file", &Kept::default());
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    open_page(window, &shell, Some("https://example.com/"), cx);
    cx.run_until_parked();
    assert!(!path.exists(), "written a moment later");

    cx.quit();

    assert_eq!(
        read_file(&path),
        main_with(&root, 0, &[page("https://example.com/", "example.com")])
    );
}

#[gpui_kit::test]
fn a_window_closed_then_the_app_quit_keeps_what_was_due_as_it_closed(cx: &mut TestAppContext) {
    let root = repo("kept-window-close-quit", &[("README.md", "hi\n")]);
    let path = file("kept-window-close-quit-file", &Kept::default());
    let (window, shell) = shell_with(&[&root], cx);
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    open_page(window, &shell, Some("https://example.com/"), cx);
    cx.run_until_parked();
    assert!(!path.exists(), "written a moment later");

    drop(shell);
    cx.update_window(window, |_, window, _| window.remove_window())
        .unwrap();
    cx.quit();

    assert_eq!(
        read_file(&path),
        main_with(&root, 0, &[page("https://example.com/", "example.com")])
    );
}

/// The window closed and opened again from the Dock, then the app quit with a write stuck on
/// a disk that stalled: quitting waits on it once, not once a window the app has had, and the
/// window open then is kept.
#[gpui_kit::test]
fn quitting_after_the_window_opened_again_waits_once_on_a_stuck_write(cx: &mut TestAppContext) {
    let root = repo("kept-window-quit-stuck", &[("README.md", "hi\n")]);
    let path = file("kept-window-quit-stuck-file", &Kept::default());
    let disk = Stuck::writing(&path.with_file_name("stuck.json"), "1");
    cx.update(|cx| cx.set_global(disk.writers.clone()));
    let (window, shell) = shell_with(&[&root], cx);
    drop(shell);
    cx.update_window(window, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    let (window, shell) = shell_window(cx);
    shell.update(cx, |shell, _| shell.workspace.add_project(root.clone()));
    launch(&shell, &path, &[&root], cx);
    select(&shell, &root, cx);
    open_page(window, &shell, Some("https://example.com/"), cx);
    cx.run_until_parked();
    assert!(!path.exists(), "written a moment later");

    let began = Instant::now();
    cx.quit();
    let waited = began.elapsed();

    assert!(waited < WAITS * 3 / 2, "quitting waited {waited:?}");
    assert_eq!(
        read_file(&path),
        main_with(&root, 0, &[page("https://example.com/", "example.com")])
    );
}

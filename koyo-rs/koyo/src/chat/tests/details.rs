//! The details card's git, with real git against a remote of its own.

use std::path::Path;
use std::process::Command;

use gpui_kit::{AnyWindowHandle, AppContext as _, Modifiers, TestAppContext, VisualTestContext};

use super::super::view::ChatView;
use super::super::view::details::{commit_files, went_old};
use super::drawing::shown_chat;
use crate::modal;
use crate::test_support::{draw, git, in_front, on_screen, scratch, write};

fn subject(folder: &Path) -> String {
    let log = Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(folder)
        .output();
    String::from_utf8(log.unwrap().stdout)
        .unwrap()
        .trim()
        .to_owned()
}

/// Whether the card has the line called `what` that does something with the branch.
fn offers(what: &'static str, window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    VisualTestContext::from_window(window, cx)
        .debug_bounds(what)
        .is_some()
}

#[gpui_kit::test]
fn the_card_commits_what_changed_and_publishes_it(cx: &mut TestAppContext) {
    let (window, view, _) = shown_chat("chat-details-git", cx);
    in_front(window, true, cx);
    let folder = view.read_with(cx, |view, _| view.details_folder()).unwrap();
    let remote = scratch("chat-details-git-remote");
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);
    git(
        &folder,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    for (setting, value) in [
        ("user.name", "Test"),
        ("user.email", "test@example.com"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/dev/null"),
    ] {
        git(&folder, &["config", setting, value]);
    }
    if !view.read_with(cx, ChatView::details_up) {
        cx.simulate_keystrokes(window, "cmd-shift-b");
    }
    view.update(cx, went_old);
    draw(window, cx);
    assert!(offers("publish", window, cx), "it's never been pushed");

    write(&folder, "new.txt", b"new\n");
    (cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| commit_files(view, 1, window, cx));
    }))
    .unwrap();
    cx.run_until_parked();
    let dialog = cx.update(|cx| modal::window(cx)).expect("the dialog is up");
    cx.simulate_keystrokes(dialog, "enter");
    cx.run_until_parked();
    assert_eq!(subject(&folder), "first", "a commit needs a message");
    cx.simulate_input(dialog, "Work");
    cx.simulate_keystrokes(dialog, "enter");
    cx.run_until_parked();
    assert_eq!(subject(&folder), "Work");
    assert!(
        cx.update(|cx| modal::window(cx)).is_none(),
        "the dialog goes once it's done"
    );

    draw(window, cx);
    let publish = VisualTestContext::from_window(window, cx).debug_bounds("publish");
    let publish = publish.expect("the card still offers to publish");
    VisualTestContext::from_window(window, cx)
        .simulate_click(publish.center(), Modifiers::default());
    draw(window, cx);
    draw(window, cx);
    assert_eq!(subject(&remote), "Work");
    assert!(!offers("publish", window, cx), "it's published");
    assert!(!offers("push", window, cx), "there's nothing to push");

    git(&folder, &["commit", "-q", "--allow-empty", "-m", "More"]);
    view.update(cx, went_old);
    draw(window, cx);
    assert!(offers("push", window, cx), "there's a commit to push");
}

/// The card reads the branch whenever its window can be seen, beside another app's too, where a
/// person watches a chat work and goes on to commit what it did. Where nobody can see the window
/// it asks the remote and GitHub nothing, and what it says of the branch stays as it was; seen
/// again, it reads the branch at once.
#[gpui_kit::test]
fn the_card_reads_the_branch_while_its_window_is_seen(cx: &mut TestAppContext) {
    let (window, view, _) = shown_chat("chat-details-seen", cx);
    in_front(window, true, cx);
    let folder = view.read_with(cx, |view, _| view.details_folder()).unwrap();
    if !view.read_with(cx, ChatView::details_up) {
        cx.simulate_keystrokes(window, "cmd-shift-b");
    }
    view.update(cx, went_old);
    draw(window, cx);
    assert!(
        !offers("publish", window, cx),
        "there's nowhere to publish to"
    );

    in_front(window, false, cx);
    let remote = scratch("chat-details-seen-remote");
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);
    git(
        &folder,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    view.update(cx, went_old);
    draw(window, cx);
    assert!(
        offers("publish", window, cx),
        "beside another app's, it reads the branch"
    );

    on_screen(window, false, cx);
    git(&folder, &["remote", "remove", "origin"]);
    view.update(cx, went_old);
    draw(window, cx);
    assert!(offers("publish", window, cx), "hidden, it reads nothing");

    on_screen(window, true, cx);
    draw(window, cx);
    assert!(
        !offers("publish", window, cx),
        "seen again, it reads the branch"
    );
}

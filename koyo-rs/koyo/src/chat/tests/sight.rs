//! A chat out of sight, its tab not shown, Settings over the workspace or its window where nobody
//! can see it, keeps up with nothing its thread does; seen again, it shows the thread as it is,
//! and what started meanwhile with how long it's run.

use std::time::Instant;

use gpui_kit::{Entity, TestAppContext};
use koyo_agent::timeline::RowKind;

use super::super::timeline::Timeline;
use super::budget::{root_of, works};
use super::chats_of;
use super::drawing::{settled, shell_with_chat};
use crate::navigation::Route;
use crate::settings_view::SettingsPage;
use crate::test_support::{draw, on_screen, open_page};

/// Whether the timeline shows a run at work, as the thread says once `works` has it.
fn shows_work(timeline: &Entity<Timeline>, cx: &TestAppContext) -> bool {
    timeline.read_with(cx, |timeline, _| {
        (timeline.rows.iter()).any(|row| matches!(row.kind, RowKind::Live { .. }))
    })
}

/// When the timeline takes what's running to have started.
fn running_since(timeline: &Entity<Timeline>, cx: &TestAppContext) -> Option<Instant> {
    timeline.read_with(cx, |timeline, _| {
        timeline.running.as_ref().map(|(_, since)| *since)
    })
}

#[gpui_kit::test]
fn a_chat_whose_tab_isnt_shown_follows_nothing_and_catches_up_once_shown(
    cx: &mut TestAppContext,
) {
    let (window, shell, view, thread) = shell_with_chat("chat-sight-tab", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let pane = shell.read_with(cx, |shell, _| shell.shown_tab_key().unwrap());
    open_page(window, &shell, Some("https://example.com/"), cx);
    draw(window, cx);

    works(&chats, &root, &thread, cx);
    draw(window, cx);
    assert!(!shows_work(&timeline, cx), "it kept up with its thread out of sight");
    let shown_at = Instant::now();

    shell.update(cx, |shell, cx| assert!(shell.show_tab_holding(pane, cx)));
    draw(window, cx);
    assert!(shows_work(&timeline, cx), "shown again, it isn't the thread as it is");
    let since = running_since(&timeline, cx).expect("what's running");
    assert!(since < shown_at, "what started out of sight runs from when it was shown");
}

#[gpui_kit::test]
fn a_chat_in_a_window_nobody_sees_follows_nothing_and_catches_up_once_seen(
    cx: &mut TestAppContext,
) {
    let (window, shell, view, thread) = shell_with_chat("chat-sight-window", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    on_screen(window, false, cx);

    works(&chats, &root, &thread, cx);
    assert!(!shows_work(&timeline, cx), "it kept up with its thread unseen");
    let seen_at = Instant::now();

    on_screen(window, true, cx);
    draw(window, cx);
    assert!(shows_work(&timeline, cx), "seen again, it isn't the thread as it is");
    let since = running_since(&timeline, cx).expect("what's running");
    assert!(since < seen_at, "what started unseen runs from when it was seen");
}

#[gpui_kit::test]
fn a_chat_behind_settings_follows_nothing_and_catches_up_once_back(cx: &mut TestAppContext) {
    let (window, shell, view, thread) = shell_with_chat("chat-sight-settings", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    shell.update(cx, |shell, cx| shell.navigate(Route::Settings(SettingsPage::General), cx));
    draw(window, cx);

    works(&chats, &root, &thread, cx);
    draw(window, cx);
    assert!(!shows_work(&timeline, cx), "it kept up with its thread behind Settings");
    let back_at = Instant::now();

    shell.update(cx, |shell, cx| shell.navigate(Route::Workspace, cx));
    draw(window, cx);
    assert!(shows_work(&timeline, cx), "back, it isn't the thread as it is");
    let since = running_since(&timeline, cx).expect("what's running");
    assert!(since < back_at, "what started behind Settings runs from when it was");
}

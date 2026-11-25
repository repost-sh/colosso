//! What a chat's own frames cost the window around it (`crate::budget`): its answer arriving,
//! its view changing and what's typed in its prompt build the chat and leave the shell alone.
//! What the sidebar shows of it, its summary, renders the shell once when it changes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, px, size};
use koyo_agent::protocol::events::Changed;
use koyo_agent::protocol::ids::{RunId, ThreadId, Time};
use koyo_agent::protocol::methods::ThreadSnapshot;
use koyo_agent::protocol::model::{
    Item, ItemStatus, Run, RunStatus, Status, ThreadSummary, ToolFamily,
};

use super::super::Chats;
use super::super::thread::{ChatThread, ThreadEvent};
use super::asks::Asking;
use super::super::view::ChatView;
use super::drawing::{settled, shell_with_chat_set, tool};
use super::{answer, appended, at, chats_of, hears, run, snapshot};
use crate::beat::IN_FRONT;
use crate::budget::{
    Built, built_after, built_again, built_for, frame_of, held_behind_and_in_front, ratchet, within,
};
use crate::settings::{self, ChatActivity};
use crate::shell::Shell;
use crate::terminals::budget::terminal_shown;
use crate::test_support::{Blank, in_front, in_window, on_screen};

const SECOND: Duration = Duration::from_secs(1);

/// The checkout the shell shows, where its chat works.
pub(super) fn root_of(shell: &Entity<Shell>, cx: &TestAppContext) -> PathBuf {
    shell.read_with(cx, |shell, _| {
        shell.workspace.branch().unwrap().worktree.path.clone()
    })
}

/// A window showing chat `0001`'s tab with the rows of `shown`, its details card down: the card
/// reads the branch whenever the window comes in front (`docs/chat.md` 5.4), which is its own
/// work, not the chat's frames these hold.
fn chat_card_down(
    name: &str,
    shown: ThreadSnapshot,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, Entity<ChatView>, Entity<ChatThread>) {
    shell_with_chat_set(name, shown, |settings| settings.chat_details.open = false, cx)
}

#[gpui_kit::test]
fn a_token_of_an_answer_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let (window, _, _, thread) = chat_card_down("budget-token", settled("0001"), cx);
    let token = |cx: &mut TestAppContext| {
        thread.update(cx, |thread, cx| {
            thread.take_in(vec![appended("a", " more")], cx);
        });
    };

    let budget = Built {
        shell: 0,
        views: 4,
        frames: 1,
    };
    within(
        built_again(window, cx, token),
        budget,
        "a token of an answer",
    );
}

/// A frame of the chat's own view, as when its timeline scrolls, which the view follows.
#[gpui_kit::test]
fn a_frame_of_a_chats_own_view_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let (window, _, view, _) = chat_card_down("budget-chat-view", settled("0001"), cx);

    let budget = Built {
        shell: 0,
        views: 1,
        frames: 1,
    };
    let built = built_again(window, cx, frame_of(&view));
    within(built, budget, "a frame of the chat's view");
}

#[gpui_kit::test]
fn what_is_typed_in_a_chats_prompt_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let (window, _, view, _) = chat_card_down("budget-typed", settled("0001"), cx);
    let composer = view.read_with(cx, |view, _| view.composer.clone());
    let text = composer.read_with(cx, |composer, _| composer.text.clone());
    let key = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            text.update(cx, |text, cx| {
                let typed = format!("{}T", text.value());
                text.set_value(typed, window, cx);
            });
        })
        .unwrap();
    };

    let budget = Built {
        shell: 0,
        views: 3,
        frames: 1,
    };
    within(built_again(window, cx, key), budget, "a key typed");
}

#[gpui_kit::test]
fn a_chats_summary_renders_the_shell_once_when_it_changes_and_not_when_it_is_the_same(
    cx: &mut TestAppContext,
) {
    let (window, shell, _, _) = chat_card_down("budget-summary", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    let unseen = ThreadSummary {
        unread: true,
        ..at(&root, Status::Failed)
    };
    let failed = |cx: &mut TestAppContext| hears(&chats, unseen.clone(), cx);

    let budget = Built {
        shell: 1,
        views: 7,
        frames: 1,
    };
    within(
        built_after(window, cx, failed),
        budget,
        "the chat's summary changed",
    );
    assert_eq!(built_after(window, cx, failed), Built::default());
}

/// What the host says of a chat that the shell doesn't show, as when it last changed and what
/// waits to be sent, renders nothing.
#[gpui_kit::test]
fn a_chats_summary_renders_nothing_when_only_what_the_shell_does_not_show_changes(
    cx: &mut TestAppContext,
) {
    let (window, shell, _, _) = chat_card_down("budget-summary-unshown", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    let later = ThreadSummary {
        updated_at: Time("2026-11-03T10:05:00.000Z".into()),
        queued: 1,
        ..at(&root, Status::Idle)
    };

    let built = built_after(window, cx, |cx| hears(&chats, later, cx));
    ratchet(
        built,
        Built::default(),
        "the chat's summary changed where the shell doesn't show it",
    );
}

/// While a chat works out of sight, as one in another checkout does while a person reads a chat
/// at rest, its spinners on its tab and its checkout's row turn at each beat (`crate::beat`), and
/// each beat builds the spinners' own views and the views around them, never the shell
/// (`chat/spinner.rs`). The chat shown defers draws outside the spinners, its details toggle
/// (`view/details.rs`) and its floating bars (`view/floating.rs`), and GPUI copies them from last
/// frame, holding nothing that changed (gpui-fast's `splice_layout`, `vendor/README.md`). Here
/// the chat shown is the one the host says works, its own thread at rest, so nothing in it moves
/// but the spinners.
#[gpui_kit::test]
fn a_chat_working_out_of_sight_builds_its_spinners_and_never_the_shell_at_each_beat(
    cx: &mut TestAppContext,
) {
    let (window, shell, _, _) = chat_card_down("budget-spinner", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    hears(&chats, at(&root, Status::Working), cx);

    let held_behind = Built {
        shell: 0,
        views: 20,
        frames: 10,
    };
    let held_in_front = Built {
        shell: 0,
        views: 60,
        frames: 30,
    };
    held_behind_and_in_front(window, SECOND, held_behind, held_in_front, cx);

    in_front(window, true, cx);
    let first = built_for(window, cx, IN_FRONT * 3);
    assert_eq!(
        first.frames, 3,
        "back in front, it turns at its pace at once"
    );

    let floating = size(px(300.), px(200.));
    let (over, _) = in_window(floating, cx, |_, cx| cx.new(|_| Blank));
    in_front(over, true, cx);
    built_for(window, cx, SECOND);
    let built = built_for(window, cx, SECOND);
    ratchet(
        built,
        held_in_front,
        "a second of it under a window of Koyo's in front",
    );
}

/// The host says chat 0001 works, as its summary and its thread do: its run is live, reading a
/// file, which the timeline's live row shows with how long it's taken.
pub(super) fn works(
    chats: &Entity<Chats>,
    root: &Path,
    thread: &Entity<ChatThread>,
    cx: &mut TestAppContext,
) {
    hears(chats, at(root, Status::Working), cx);
    let reading = Item {
        run_id: RunId("r2".into()),
        body: tool(ToolFamily::Read, "b.rs", None),
        ..answer("w", 5, "", 20)
    };
    let running = run("r2", RunStatus::Running, "Go on.", 20);
    thread.update(cx, |thread, cx| {
        thread.take_in(vec![changed(running, reading)], cx);
    });
}

/// The host says chat 0001 is done: its run and what it read have finished.
fn stops(chats: &Entity<Chats>, root: &Path, thread: &Entity<ChatThread>, cx: &mut TestAppContext) {
    hears(chats, at(root, Status::Idle), cx);
    let read = Item {
        run_id: RunId("r2".into()),
        status: ItemStatus::Completed,
        body: tool(ToolFamily::Read, "b.rs", None),
        ..answer("w", 5, "", 21)
    };
    let done = run("r2", RunStatus::Completed, "Go on.", 21);
    thread.update(cx, |thread, cx| {
        thread.take_in(vec![changed(done, read)], cx);
    });
}

/// Chat 0001 at `run`'s sequence, with `run` and `item` as they are then.
fn changed(run: Run, item: Item) -> ThreadEvent {
    ThreadEvent::Changed(Box::new(Changed {
        thread_id: ThreadId("0001".into()),
        seq: run.seq,
        thread: Some(snapshot("0001", run.seq, Vec::new()).thread),
        runs: vec![run],
        items: vec![item],
        requests: Vec::new(),
    }))
}

/// A chat at work moves in three ways: its spinners turn, its composer's edge goes round, and the
/// live row's highlight sweeps over what it's doing (`chat/rows.rs`). Behind another app's window
/// only the first two do, on the beat. The edge stepping, the shown chat's frames are its own and
/// the shell's alone.
#[gpui_kit::test]
fn a_working_chat_draws_its_spinners_edge_and_live_row(cx: &mut TestAppContext) {
    let (window, shell, _, thread) = chat_card_down("budget-working", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    works(&chats, &root, &thread, cx);

    let held_behind = Built {
        shell: 0,
        views: 43,
        frames: 11,
    };
    let held_in_front = Built {
        shell: 0,
        views: 180,
        frames: 50,
    };
    held_behind_and_in_front(window, SECOND, held_behind, held_in_front, cx);
}

/// With the edge held still or turned off in Settings, nothing in the shown chat steps with the
/// spinners, and each beat builds the spinners and leaves the shell alone, as for a chat at work
/// out of sight.
#[gpui_kit::test]
fn a_working_chat_with_its_edge_still_or_off_leaves_the_shell_alone_at_each_beat(
    cx: &mut TestAppContext,
) {
    let (window, shell, _, thread) = chat_card_down("budget-edge-still", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    let held_behind = Built {
        shell: 0,
        views: 23,
        frames: 11,
    };
    let held_in_front = Built {
        shell: 0,
        views: 120,
        frames: 50,
    };

    cx.update(|cx| settings::update(cx, |s| s.chat_activity = ChatActivity::Still));
    works(&chats, &root, &thread, cx);
    held_behind_and_in_front(window, SECOND, held_behind, held_in_front, cx);

    cx.update(|cx| settings::update(cx, |s| s.chat_activity = ChatActivity::Off));
    held_behind_and_in_front(window, SECOND, held_behind, held_in_front, cx);
}

/// Once a chat stops working and its edge has faded out, nothing in the window asks for a beat.
fn stops_the_beat(name: &str, front: bool, cx: &mut TestAppContext) {
    let (window, shell, _, thread) = chat_card_down(name, settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    in_front(window, front, cx);
    works(&chats, &root, &thread, cx);
    built_for(window, cx, SECOND);

    stops(&chats, &root, &thread, cx);
    built_for(window, cx, 2 * SECOND);
    let built = built_for(window, cx, 2 * SECOND);
    assert_eq!(built, Built::default(), "two seconds after it stopped");
}

#[gpui_kit::test]
fn a_chat_that_stops_working_in_front_stops_the_beat(cx: &mut TestAppContext) {
    stops_the_beat("budget-stop-front", true, cx);
}

#[gpui_kit::test]
fn a_chat_that_stops_working_behind_another_apps_window_stops_the_beat(cx: &mut TestAppContext) {
    stops_the_beat("budget-stop-behind", false, cx);
}

#[gpui_kit::test]
fn a_chat_that_stops_working_with_reduced_motion_stops_the_beat(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    stops_the_beat("budget-stop-still", true, cx);
}

/// A terminal shown in the checkout a chat works in: the chat's spinners on its tab and the
/// checkout's row turn beside it, and as nothing the terminal draws is deferred, each beat builds
/// the spinners alone.
#[gpui_kit::test]
fn a_terminal_shown_beside_a_working_chats_spinners_leaves_the_shell_alone(
    cx: &mut TestAppContext,
) {
    let (window, shell, _, thread) = chat_card_down("budget-terminal-beside", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    works(&chats, &root, &thread, cx);
    terminal_shown(window, &shell, &root, cx);

    let held_behind = Built {
        shell: 0,
        views: 20,
        frames: 10,
    };
    let held_in_front = Built {
        shell: 0,
        views: 60,
        frames: 30,
    };
    held_behind_and_in_front(window, SECOND, held_behind, held_in_front, cx);
}

/// A window nobody can see (covered, minimized, on another desktop) asks for no beat and no tick,
/// so a chat at work in it builds nothing; seen again, it's drawn whole and goes on at once.
#[gpui_kit::test]
fn a_working_chat_in_a_window_nobody_sees_builds_nothing(cx: &mut TestAppContext) {
    let (window, shell, _, thread) = chat_card_down("budget-hidden", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    works(&chats, &root, &thread, cx);
    in_front(window, false, cx);

    on_screen(window, false, cx);
    built_for(window, cx, SECOND);
    let built = built_for(window, cx, 2 * SECOND);
    assert_eq!(built, Built::default(), "two seconds of it hidden");

    on_screen(window, true, cx);
    let built = built_for(window, cx, SECOND);
    assert!(built.frames >= 10, "seen again, it goes on: {built:?}");
}

/// The harness drives a Koyo on a desktop nobody looks at (`tick`, `crate::sight`): a window there
/// counts as seen, so a working chat's spinners go on turning at each beat (`crate::beat`).
#[gpui_kit::test]
fn a_working_chat_goes_on_in_a_window_the_harness_counts_as_seen(cx: &mut TestAppContext) {
    let (window, shell, _, thread) = chat_card_down("budget-unseen-counts", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    works(&chats, &root, &thread, cx);
    in_front(window, false, cx);
    crate::sight::count_unseen(true);
    on_screen(window, false, cx);
    built_for(window, cx, SECOND);
    let built = built_for(window, cx, SECOND);
    crate::sight::count_unseen(false);
    assert!(built.frames >= 10, "counted seen, it goes on: {built:?}");
}

/// What's running shows how long it's taken, the timeline built again each second for it
/// (`chat/timeline.rs`), with reduced motion holding all else still: behind another app's window
/// as in front, where a person watches it from elsewhere, and not at all where nobody can see it.
#[gpui_kit::test]
fn a_running_items_time_builds_the_timeline_each_second_where_it_is_seen(cx: &mut TestAppContext) {
    let (window, shell, view, thread) = chat_card_down("budget-tick", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    cx.update(|cx| cx.set_reduce_motion(true));
    works(&chats, &root, &thread, cx);
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let drawn = |cx: &TestAppContext| timeline.read_with(cx, |timeline, _| timeline.renders);

    let held_behind = Built {
        shell: 0,
        views: 7,
        frames: 2,
    };
    let held_in_front = Built {
        shell: 0,
        views: 6,
        frames: 2,
    };
    held_behind_and_in_front(window, 2 * SECOND, held_behind, held_in_front, cx);

    on_screen(window, false, cx);
    built_for(window, cx, SECOND);
    let built = built_for(window, cx, 2 * SECOND);
    assert_eq!(built, Built::default(), "two seconds of it hidden");
    let was = drawn(cx);
    on_screen(window, true, cx);
    assert!(drawn(cx) > was, "drawn as it is now once it's seen again");
    ratchet(built_for(window, cx, 2 * SECOND), held_behind, "seen again");
}

/// The composer's caret blinks while it has the keyboard, each turn building the composer
/// (`chat/composer.rs`), which looks for a change to its draft then. It isn't drawn behind another
/// app's window.
#[gpui_kit::test]
fn a_focused_composers_caret_builds_the_composer_at_each_blink(cx: &mut TestAppContext) {
    let chat = Asking::new("budget-caret", Vec::new(), cx);
    let window = chat.window;
    let two_seconds = |cx: &mut TestAppContext| built_for(window, cx, 2 * SECOND);

    in_front(window, true, cx);
    built_for(window, cx, 3 * SECOND);
    let held_blinking = Built {
        shell: 0,
        views: 12,
        frames: 4,
    };
    ratchet(two_seconds(cx), held_blinking, "two seconds of it in front");

    in_front(window, false, cx);
    ratchet(
        two_seconds(cx),
        Built::default(),
        "two seconds of it behind another app's",
    );
    in_front(window, true, cx);
    ratchet(two_seconds(cx), held_blinking, "two seconds of it in front again");
}

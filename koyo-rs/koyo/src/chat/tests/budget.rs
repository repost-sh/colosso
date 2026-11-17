//! What a chat's own frames cost the window around it (`crate::budget`): its answer arriving,
//! its view changing and what's typed in its prompt build the chat and leave the shell alone.
//! What the sidebar shows of it, its summary, renders the shell once when it changes.

use std::path::PathBuf;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use koyo_agent::protocol::model::Status;

use super::drawing::{settled, shell_with_chat};
use super::{appended, at, chats_of, hears};
use crate::budget::{Built, built_after, built_again, built_over, frame_of, within};
use crate::shell::Shell;

/// The checkout the shell shows, where its chat works.
fn root_of(shell: &Entity<Shell>, cx: &TestAppContext) -> PathBuf {
    shell.read_with(cx, |shell, _| {
        shell.workspace.branch().unwrap().worktree.path.clone()
    })
}

#[gpui_kit::test]
fn a_token_of_an_answer_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let (window, _, _, thread) = shell_with_chat("budget-token", settled("0001"), cx);
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
    let (window, _, view, _) = shell_with_chat("budget-chat-view", settled("0001"), cx);

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
    let (window, _, view, _) = shell_with_chat("budget-typed", settled("0001"), cx);
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
    let (window, shell, _, _) = shell_with_chat("budget-summary", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    let failed = |cx: &mut TestAppContext| hears(&chats, at(&root, Status::Failed), cx);

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

/// A ratchet, not the budget wanted: while a chat works, each frame of its spinner on its tab or
/// its checkout's row renders the shell, where the spinner's own view was meant to be all a
/// frame builds (`chat/spinner.rs`). GPUI draws a view around the views nested in it that
/// changed only if every deferred draw in it is inside one of those (gpui-fast's
/// `splice_layout`), and the chat shown draws some outside the spinners: its details toggle
/// (`view/details.rs`) and its floating bars (`view/floating.rs`). Drawn without `deferred`,
/// the shell renders in none of these frames. It's held here at what it costs, so it gets no
/// worse unseen, and this fails once it's fixed: the budget is then none of the frames.
#[gpui_kit::test]
fn a_working_chats_spinner_renders_the_shell_at_each_of_its_frames(cx: &mut TestAppContext) {
    let (window, shell, _, _) = shell_with_chat("budget-spinner", settled("0001"), cx);
    let (chats, root) = (chats_of(&shell, cx), root_of(&shell, cx));
    hears(&chats, at(&root, Status::Working), cx);
    built_over(window, cx, 20);

    let budget = Built {
        shell: 60,
        views: 240,
        frames: 60,
    };
    within(
        built_over(window, cx, 60),
        budget,
        "a second of the spinner",
    );
}

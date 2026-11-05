//! The rows a chat's records make in the tab's list, and what each change draws.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext};
use koyo_agent::protocol::ids::{ItemId, RunId, ThreadId, Time};
use koyo_agent::protocol::methods::ThreadSnapshot;
use koyo_agent::protocol::model::{
    Delivered, Item, ItemBody, ItemStatus, Run, RunStatus, Text, Tool, ToolFamily,
    UserMessage,
};
use koyo_agent::timeline::RowId;

use super::super::thread::ChatThread;
use super::super::timeline::OpenSubagent;
use super::super::view::ChatView;
use super::{AT, answer, appended, chats_of, run, snapshot, summary, text_of};
use crate::settings::{self, ChatWork};
use crate::test_support::{draw, repo, select, shell_with};

/// A run that's over, with a prompt, a tool and an answer.
fn settled(id: &str) -> ThreadSnapshot {
    let run = Run {
        thread_id: ThreadId(id.into()),
        started_at: Some(Time(AT.into())),
        ended_at: Some(Time(AT.into())),
        ..run("r", RunStatus::Completed, "Why?", 3)
    };
    let message = run.prompt.message_id.clone();
    let done = |id: &str, ordinal: u64, body: ItemBody| Item {
        status: ItemStatus::Completed,
        body,
        ..answer(id, ordinal, "", ordinal)
    };
    let items = vec![
        done(
            "u",
            0,
            ItemBody::UserMessage(UserMessage {
                message_id: message,
                text: "Why?".into(),
                attachments: Vec::new(),
                delivered: Delivered::Turn,
            }),
        ),
        done(
            "t",
            1,
            ItemBody::Tool(Tool {
                name: "Read".into(),
                family: ToolFamily::Read,
                label: "a.rs".into(),
                input: serde_json::Value::Null,
                output: None,
                exit_code: None,
                patch: None,
                files: Vec::new(),
                server: None,
            }),
        ),
        done("a", 2, ItemBody::AssistantText(Text { text: "Because".into() })),
    ];
    ThreadSnapshot {
        runs: vec![run],
        ..snapshot(id, 3, items)
    }
}

/// A window showing chat `0001`'s tab, with the rows of `settled`.
pub(super) fn shown_chat(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<ChatView>, Entity<ChatThread>) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let chats = chats_of(&shell, cx);
    chats.update(cx, |chats, cx| {
        chats.listed(Ok(vec![summary("0001", &root, "A chat", true)]), cx);
    });
    draw(window, cx);
    let view = shell.read_with(cx, |shell, _| {
        let pane = shell.workspace.branch().unwrap().tabs[0].first().id;
        shell.chat_tabs.view(pane).unwrap().clone()
    });
    let thread = chats.update(cx, |chats, cx| chats.thread(&ThreadId("0001".into()), cx));
    thread.update(cx, |thread, cx| {
        thread.opening();
        thread.opened(Ok(settled("0001")), cx);
    });
    draw(window, cx);
    (window, view, thread)
}

/// A row's height is measured once, so without measuring again the next would still start where
/// the row ended at the old size, under its text.
#[gpui_kit::test]
fn a_new_text_size_grows_the_rows_and_measures_them_again(cx: &mut TestAppContext) {
    let (window, _view, _thread) = shown_chat("chat-text-size", cx);
    let bounds = |at: &'static str, cx: &mut TestAppContext| {
        VisualTestContext::from_window(window, cx).debug_bounds(at).unwrap()
    };
    let before = bounds("chat-row-0", cx);
    cx.update(|cx| settings::update(cx, |settings| settings.fonts.chat_size = 22.));
    draw(window, cx);
    let (grown, next) = (bounds("chat-row-0", cx), bounds("chat-row-1", cx));
    assert!(grown.size.height > before.size.height, "{before:?} to {grown:?}");
    assert!(next.origin.y >= grown.origin.y + grown.size.height, "{grown:?} over {next:?}");
}

/// A draft wraps differently at another size, and a new size is neither an edit nor a new width,
/// the things that count its rows. Only the line's height growing would leave some of it out of
/// sight, in a box that was fitted to the old wrapping.
#[gpui_kit::test]
fn a_new_text_size_fits_the_composer_to_its_draft_again(cx: &mut TestAppContext) {
    let (window, view, _thread) = shown_chat("chat-draft-size", cx);
    let text = view.read_with(cx, |view, cx| view.composer.read(cx).text.clone());
    cx.update_window(window, |_, window, cx| {
        text.update(cx, |text, cx| text.set_value("word ".repeat(70), window, cx));
    })
    .unwrap();
    draw(window, cx);
    let height = |cx: &TestAppContext| {
        text.read_with(cx, |text, _| f32::from(text.input_bounds().size.height))
    };
    let at_16 = height(cx);
    cx.update(|cx| settings::update(cx, |settings| settings.fonts.chat_size = 22.));
    draw(window, cx);
    let at_22 = height(cx);
    // Its line goes from 20 to 28, rounded, so the same rows would only reach 1.4 times the height.
    assert!(at_22 >= at_16 * 1.5, "more rows too: {at_16} to {at_22}");
}

#[gpui_kit::test]
fn the_list_holds_a_row_for_what_shows_and_a_fold_or_a_token_changes_only_that(
    cx: &mut TestAppContext,
) {
    let (window, view, thread) = shown_chat("chat-rows", cx);
    let rows = |cx: &TestAppContext| {
        view.read_with(cx, |view, cx| view.showing().unwrap().scroller.read(cx).item_count())
    };
    assert_eq!(rows(cx), 3, "the prompt, its work and the answer");
    cx.update(|cx| settings::update(cx, |settings| settings.chat_work = ChatWork::Turns));
    draw(window, cx);
    assert_eq!(rows(cx), 3, "the prompt, the fold and the answer");

    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let fold = RowId::Fold(RunId("r".into()));
    timeline.update(cx, |timeline, cx| timeline.toggle(fold, 1, cx));
    draw(window, cx);
    assert_eq!(rows(cx), 4, "and the work the fold held");

    thread.update(cx, |thread, cx| {
        thread.take_in(vec![appended("a", " of that")], cx);
    });
    draw(window, cx);
    assert_eq!(rows(cx), 4);
    assert!(text_of(&thread, cx).contains("Because of that"));
}

#[gpui_kit::test]
fn how_much_of_the_work_shows_is_chosen_and_a_click_still_has_the_last_word(cx: &mut TestAppContext) {
    let (window, view, _) = shown_chat("chat-work", cx);
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let line = RowId::Item(ItemId("t".into()));
    let is_open = |cx: &mut TestAppContext| {
        timeline.update(cx, |timeline, cx| timeline.drawing(cx).work_open(&line))
    };
    let click = |cx: &mut TestAppContext| {
        timeline.update(cx, |timeline, cx| timeline.toggle(line.clone(), 1, cx));
    };
    let choose = |work, cx: &mut TestAppContext| {
        cx.update(|cx| settings::update(cx, |settings| settings.chat_work = work));
        draw(window, cx);
    };
    let has_fold = |cx: &mut TestAppContext| {
        timeline.read_with(cx, |timeline, _| {
            timeline.rows.iter().any(|row| matches!(row.id, RowId::Fold(_)))
        })
    };

    assert!(!has_fold(cx), "nothing folds unless that's what's chosen");
    assert!(!is_open(cx), "and a line waits to be opened");
    click(cx);
    assert!(is_open(cx));

    choose(ChatWork::Everything, cx);
    assert!(!has_fold(cx));
    assert!(is_open(cx), "all of it, and the one opened by hand doesn't now read as closed");
    click(cx);
    assert!(!is_open(cx), "one can still be closed");

    choose(ChatWork::Steps, cx);
    assert!(!is_open(cx), "the one closed by hand doesn't now read as opened");

    choose(ChatWork::Turns, cx);
    assert!(has_fold(cx));
}

/// The budget's two sides (`docs/chat.md` 5.4): a token draws no composer, and what's typed no row.
/// Counted over the frames that come by themselves: `draw` forces one more than anything asked for.
#[gpui_kit::test]
fn a_token_draws_the_timeline_alone_and_whats_typed_the_composer_alone(cx: &mut TestAppContext) {
    let (window, view, thread) = shown_chat("chat-budget", cx);
    let (composer, timeline) = view.read_with(cx, |view, _| {
        (view.composer.clone(), view.showing().unwrap().timeline.clone())
    });
    let text = composer.read_with(cx, |composer, _| composer.text.clone());
    let drawn = |cx: &TestAppContext| {
        let typed = composer.read_with(cx, |composer, _| composer.renders);
        (typed, timeline.read_with(cx, |timeline, _| timeline.renders))
    };
    let token = |cx: &mut TestAppContext| {
        thread.update(cx, |thread, cx| {
            thread.take_in(vec![appended("a", " more")], cx);
        });
        cx.run_until_parked();
    };
    let key = |value: &str, cx: &mut TestAppContext| {
        let value = value.to_owned();
        cx.update_window(window, |_, window, cx| {
            text.update(cx, |text, cx| text.set_value(value, window, cx));
        })
        .unwrap();
        cx.run_until_parked();
    };
    token(cx);

    let (typed, rows) = drawn(cx);
    token(cx);
    token(cx);
    token(cx);
    assert_eq!(drawn(cx), (typed, rows + 3), "each token draws the rows, none the composer");

    let (typed, rows) = drawn(cx);
    key("T", cx);
    key("Ty", cx);
    key("Typ", cx);
    assert_eq!(drawn(cx), (typed + 3, rows), "each key draws the composer, none the rows");
}

#[gpui_kit::test]
fn a_subagents_chat_goes_over_its_parent_and_is_let_go_of_coming_back(cx: &mut TestAppContext) {
    let (window, view, parent) = shown_chat("chat-subagent", cx);
    let child = ThreadId("0002".into());
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    timeline.update(cx, |_, cx| cx.emit(OpenSubagent(child.clone())));
    draw(window, cx);
    let chats = view.read_with(cx, |view, _| view.chats.clone());
    let showing = |cx: &TestAppContext| {
        view.read_with(cx, |view, cx| view.showing().unwrap().thread.read(cx).id().clone())
    };
    assert_eq!(showing(cx), child);
    assert!(chats.read_with(cx, |chats, _| chats.threads.contains_key(&child)));
    assert!(view.read_with(cx, |view, _| view.composer_on_show().is_none()), "a bar's in its place");

    cx.simulate_keystrokes(window, "escape");
    draw(window, cx);
    assert_eq!(showing(cx), parent.read_with(cx, |parent, _| parent.id().clone()));
    assert!(view.read_with(cx, |view, _| view.composer_on_show().is_some()));
    assert!(!chats.read_with(cx, |chats, _| chats.threads.contains_key(&child)), "nothing shows it");
}

#[gpui_kit::test]
fn the_details_card_comes_up_and_goes_by_its_key_wherever_the_keys_are_in_the_chat(
    cx: &mut TestAppContext,
) {
    let (window, view, _) = shown_chat("chat-details", cx);
    let up = |cx: &TestAppContext| view.read_with(cx, ChatView::details_up);
    let before = up(cx);
    cx.simulate_keystrokes(window, "cmd-shift-b");
    draw(window, cx);
    assert_eq!(up(cx), !before);
    cx.simulate_keystrokes(window, "cmd-shift-b");
    draw(window, cx);
    assert_eq!(up(cx), before);
}

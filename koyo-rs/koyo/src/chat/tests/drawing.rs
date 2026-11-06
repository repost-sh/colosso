//! The rows a chat's records make in the tab's list, and what each change draws.

use gpui_kit::base::TextSelection;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton, TestAppContext,
    VisualTestContext, point, px,
};
use koyo_agent::protocol::ids::{ItemId, RunId, ThreadId, Time};
use koyo_agent::protocol::methods::ThreadSnapshot;
use koyo_agent::protocol::model::{
    Delivered, Hunk, Item, ItemBody, ItemStatus, Run, RunStatus, Text, Tool, ToolFamily,
    UserMessage,
};
use koyo_agent::timeline::RowId;

use super::super::thread::ChatThread;
use super::super::timeline::{OpenSubagent, Timeline};
use super::super::view::ChatView;
use super::{AT, answer, appended, chats_of, run, snapshot, summary, text_of};
use crate::settings::{self, CHAT_SIZES, LARGEST_CHAT_SIZE, Preset, Shows, Step, Verbosity};
use crate::shell::Shell;
use crate::test_support::{draw, repo, select, shell_with};

/// A tool that read `label`, or changed it by `patch`.
fn tool(family: ToolFamily, label: &str, patch: Option<Vec<Hunk>>) -> ItemBody {
    ItemBody::Tool(Tool {
        name: "Tool".into(),
        family,
        label: label.into(),
        input: serde_json::Value::Null,
        output: None,
        exit_code: None,
        patch,
        files: Vec::new(),
        server: None,
    })
}

/// A run that's over, with a prompt, a tool and an answer.
pub(super) fn settled(id: &str) -> ThreadSnapshot {
    settled_with(id, Vec::new())
}

/// A run that's over, with a prompt, a tool, then `more` work, and an answer.
pub(super) fn settled_with(id: &str, more: Vec<(&str, ItemBody)>) -> ThreadSnapshot {
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
    let mut items = vec![
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
        done("t", 1, tool(ToolFamily::Read, "a.rs", None)),
    ];
    let after = (2..)
        .zip(more)
        .map(|(ordinal, (id, body))| done(id, ordinal, body));
    items.extend(after);
    let last = items.len() as u64;
    items.push(done(
        "a",
        last,
        ItemBody::AssistantText(Text {
            text: "Because".into(),
        }),
    ));
    ThreadSnapshot {
        runs: vec![run],
        ..snapshot(id, last + 1, items)
    }
}

/// A window showing chat `0001`'s tab, with the rows of `settled`.
pub(super) fn shown_chat(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<ChatView>, Entity<ChatThread>) {
    let (window, _, view, thread) = shell_with_chat(name, settled("0001"), cx);
    (window, view, thread)
}

/// How much of item `id` of `timeline`'s work shows.
pub(super) fn shows_of(timeline: &Entity<Timeline>, id: &str, cx: &mut TestAppContext) -> Shows {
    timeline.update(cx, |timeline, cx| {
        let replica = timeline.thread.read(cx).replica().unwrap();
        let item = replica
            .items
            .iter()
            .find(|item| item.id.0 == id)
            .unwrap()
            .clone();
        timeline
            .drawing(cx)
            .shows(&item, &RowId::Item(item.id.clone()))
    })
}

/// The shell of a window showing chat `0001`'s tab, with the rows of `shown`.
pub(super) fn shell_with_chat(
    name: &str,
    shown: ThreadSnapshot,
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<Shell>,
    Entity<ChatView>,
    Entity<ChatThread>,
) {
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
        thread.opened(Ok(shown), cx);
    });
    draw(window, cx);
    (window, shell, view, thread)
}

/// A row's height is measured once, so without measuring again the next would still start where
/// the row ended at the old size, under its text.
#[gpui_kit::test]
fn a_new_text_size_grows_the_rows_and_measures_them_again(cx: &mut TestAppContext) {
    let (window, _view, _thread) = shown_chat("chat-text-size", cx);
    let bounds = |at: &'static str, cx: &mut TestAppContext| {
        VisualTestContext::from_window(window, cx)
            .debug_bounds(at)
            .unwrap()
    };
    let before = bounds("chat-row-0", cx);
    cx.update(|cx| settings::update(cx, |settings| settings.fonts.chat_size = LARGEST_CHAT_SIZE));
    draw(window, cx);
    let (grown, next) = (bounds("chat-row-0", cx), bounds("chat-row-1", cx));
    assert!(
        grown.size.height > before.size.height,
        "{before:?} to {grown:?}"
    );
    assert!(
        next.origin.y >= grown.origin.y + grown.size.height,
        "{grown:?} over {next:?}"
    );
}

/// A draft wraps differently at another size, and a new size is neither an edit nor a new width,
/// the things that count its rows. Only the line's height growing would leave some of it out of
/// sight, in a box that was fitted to the old wrapping.
#[gpui_kit::test]
fn a_new_text_size_fits_the_composer_to_its_draft_again(cx: &mut TestAppContext) {
    let (window, view, _thread) = shown_chat("chat-draft-size", cx);
    let text = view.read_with(cx, |view, cx| view.composer.read(cx).text.clone());
    cx.update_window(window, |_, window, cx| {
        text.update(cx, |text, cx| {
            text.set_value("word ".repeat(70), window, cx);
        });
    })
    .unwrap();
    cx.update(|cx| settings::update(cx, |settings| settings.fonts.chat_size = CHAT_SIZES[0]));
    draw(window, cx);
    let height = |cx: &TestAppContext| {
        text.read_with(cx, |text, _| f32::from(text.input_bounds().size.height))
    };
    let smallest = height(cx);
    cx.update(|cx| settings::update(cx, |settings| settings.fonts.chat_size = LARGEST_CHAT_SIZE));
    draw(window, cx);
    let largest = height(cx);
    // Its line goes from 14 to 23, rounded, so the same rows would only reach 1.65 times the
    // height.
    assert!(
        largest >= smallest * 1.8,
        "more rows too: {smallest} to {largest}"
    );
}

#[gpui_kit::test]
fn the_list_holds_a_row_for_what_shows_and_a_fold_or_a_token_changes_only_that(
    cx: &mut TestAppContext,
) {
    let (window, view, thread) = shown_chat("chat-rows", cx);
    let rows = |cx: &TestAppContext| {
        view.read_with(cx, |view, cx| {
            view.showing().unwrap().scroller.read(cx).item_count()
        })
    };
    assert_eq!(rows(cx), 3, "the prompt, its work and the answer");
    let quiet = Preset::Quiet.verbosity();
    cx.update(|cx| settings::update(cx, |settings| settings.chat_verbosity = quiet));
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
fn how_much_of_the_work_shows_is_chosen_and_a_click_still_has_the_last_word(
    cx: &mut TestAppContext,
) {
    let (window, view, _) = shown_chat("chat-work", cx);
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let line = RowId::Item(ItemId("t".into()));
    let shows = |cx: &mut TestAppContext| shows_of(&timeline, "t", cx);
    let click = |cx: &mut TestAppContext| {
        timeline.update(cx, |timeline, cx| timeline.toggle(line.clone(), 1, cx));
    };
    let choose = |verbosity: Verbosity, cx: &mut TestAppContext| {
        cx.update(|cx| settings::update(cx, |settings| settings.chat_verbosity = verbosity));
        draw(window, cx);
    };
    let has_fold = |cx: &mut TestAppContext| {
        timeline.read_with(cx, |timeline, _| {
            timeline
                .rows
                .iter()
                .any(|row| matches!(row.id, RowId::Fold(_)))
        })
    };

    assert!(!has_fold(cx), "nothing folds unless that's what's chosen");
    assert_eq!(shows(cx), Shows::Line, "and a file read waits to be opened");
    click(cx);
    assert_eq!(shows(cx), Shows::All);

    choose(Preset::Verbose.verbosity(), cx);
    assert!(!has_fold(cx));
    assert_eq!(
        shows(cx),
        Shows::All,
        "all of it, and the one opened by hand doesn't now read as closed"
    );
    click(cx);
    assert_eq!(shows(cx), Shows::Line, "one can still be closed");

    choose(Verbosity::default().with(Step::Results, Shows::Preview), cx);
    assert_eq!(
        shows(cx),
        Shows::Preview,
        "the one closed by hand doesn't now read as opened"
    );
    click(cx);
    assert_eq!(shows(cx), Shows::All, "a few lines open on all of it");

    choose(Preset::Quiet.verbosity(), cx);
    assert!(has_fold(cx));
}

/// As Claude Code shows it: an edit is a row of its own with its diff, which is coloured once
/// that's worked out off the main thread, while what was read stays on one line.
#[gpui_kit::test]
fn an_edit_shows_its_diff_on_a_row_of_its_own_and_is_coloured_once_that_is_worked_out(
    cx: &mut TestAppContext,
) {
    let patch = vec![Hunk {
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 1,
        lines: vec!["-fn old() {}".into(), "+fn new() {}".into()],
    }];
    let edit = tool(ToolFamily::Edit, "src/main.rs", Some(patch));
    let more = vec![("e", edit), ("r", tool(ToolFamily::Read, "b.rs", None))];
    let (window, _, view, _) = shell_with_chat("chat-edit", settled_with("0001", more), cx);
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let rows = |cx: &mut TestAppContext| {
        timeline.read_with(cx, |timeline, _| {
            timeline
                .rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        })
    };
    let work = |id: &str| RowId::Work(ItemId(id.into()));
    assert_eq!(rows(cx)[1..4], [work("t"), work("e"), work("r")]);
    cx.run_until_parked();
    let coloured = timeline.read_with(cx, |timeline, _| {
        timeline.colours.contains_key(&ItemId("e".into()))
    });
    assert!(coloured, "its colours are worked out");

    // A drag over its two lines copies their code, without the numbers beside it.
    draw(window, cx);
    let code = |line: &str, cx: &mut TestAppContext| {
        let selector = format!("diff-e-0-{line}-code");
        VisualTestContext::from_window(window, cx)
            .debug_bounds(selector.leak())
            .unwrap()
    };
    let (first, second) = (code("0", cx), code("1", cx));
    let mut visual = VisualTestContext::from_window(window, cx);
    let none = Modifiers::default();
    // From just in from the first line's code, which starts past its padding, to past the
    // second's end.
    let from = first.origin + point(px(9.), first.size.height / 2.);
    let to = second.origin + point(second.size.width - px(2.), second.size.height / 2.);
    visual.simulate_mouse_down(from, MouseButton::Left, none);
    visual.simulate_mouse_move(to, Some(MouseButton::Left), none);
    visual.simulate_mouse_up(to, MouseButton::Left, none);
    draw(window, cx);
    let copied = cx.update_window(window, |_, window, cx| {
        TextSelection::selected_text(window, cx)
    });
    assert_eq!(copied.unwrap(), "fn old() {}\nfn new() {}");

    let quiet = Preset::Quiet.verbosity();
    cx.update(|cx| settings::update(cx, |settings| settings.chat_verbosity = quiet));
    draw(window, cx);
    assert_eq!(rows(cx).len(), 3, "the prompt, the fold and the answer");
}

/// The budget's two sides (`docs/chat.md` 5.4): a token draws no composer, and what's typed no row.
/// Counted over the frames that come by themselves: `draw` forces one more than anything asked for.
#[gpui_kit::test]
fn a_token_draws_the_timeline_alone_and_whats_typed_the_composer_alone(cx: &mut TestAppContext) {
    let (window, view, thread) = shown_chat("chat-budget", cx);
    let (composer, timeline) = view.read_with(cx, |view, _| {
        (
            view.composer.clone(),
            view.showing().unwrap().timeline.clone(),
        )
    });
    let text = composer.read_with(cx, |composer, _| composer.text.clone());
    let drawn = |cx: &TestAppContext| {
        let typed = composer.read_with(cx, |composer, _| composer.renders);
        (
            typed,
            timeline.read_with(cx, |timeline, _| timeline.renders),
        )
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
    assert_eq!(
        drawn(cx),
        (typed, rows + 3),
        "each token draws the rows, none the composer"
    );

    let (typed, rows) = drawn(cx);
    key("T", cx);
    key("Ty", cx);
    key("Typ", cx);
    assert_eq!(
        drawn(cx),
        (typed + 3, rows),
        "each key draws the composer, none the rows"
    );
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
        view.read_with(cx, |view, cx| {
            view.showing().unwrap().thread.read(cx).id().clone()
        })
    };
    assert_eq!(showing(cx), child);
    assert!(chats.read_with(cx, |chats, _| chats.threads.contains_key(&child)));
    assert!(
        view.read_with(cx, |view, _| view.composer_on_show().is_none()),
        "a bar's in its place"
    );

    cx.simulate_keystrokes(window, "escape");
    draw(window, cx);
    assert_eq!(
        showing(cx),
        parent.read_with(cx, |parent, _| parent.id().clone())
    );
    assert!(view.read_with(cx, |view, _| view.composer_on_show().is_some()));
    assert!(
        !chats.read_with(cx, |chats, _| chats.threads.contains_key(&child)),
        "nothing shows it"
    );
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

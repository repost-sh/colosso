//! What a person said, in the timeline: markdown drawn as an answer's is, a long message cut to a
//! few lines until it's opened, and a picture it attached opening in Koyo's viewer over the chat.

use gpui_kit::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext};
use koyo_agent::protocol::ids::ItemId;
use koyo_agent::protocol::model::{Attachment, ItemBody};

use super::super::thread::ChatThread;
use super::super::timeline::OpenPicture;
use super::super::view::ChatView;
use super::drawing::{settled_with, shell_with_chat};
use crate::test_support::{draw, scratch};

/// What the view of item `id`'s text renders, once it has drawn one.
fn rendered(thread: &Entity<ChatThread>, id: &str, cx: &TestAppContext) -> Option<String> {
    thread.read_with(cx, |thread, cx| {
        let text = thread.drawn_text(&ItemId(id.into()))?;
        Some(text.read(cx).rendered_text().as_str().to_owned())
    })
}

/// A chat whose person said `text`.
fn saying(
    name: &str,
    text: &str,
    cx: &mut TestAppContext,
) -> (
    gpui_kit::AnyWindowHandle,
    Entity<ChatView>,
    Entity<ChatThread>,
) {
    let mut shown = settled_with("0001", Vec::new());
    if let ItemBody::UserMessage(message) = &mut shown.items[0].body {
        message.text = text.into();
    }
    let (window, _, view, thread) = shell_with_chat(name, shown, cx);
    draw(window, cx);
    (window, view, thread)
}

#[gpui_kit::test]
fn a_persons_message_is_markdown_drawn_as_an_answer_is(cx: &mut TestAppContext) {
    let said = "## Plan\n\n**Read** `main.rs`, then *check* it:\n\n- one\n- [docs](https://koyo.dev)\n\n> quoted";
    let (_, _, thread) = saying("chat-said-markdown", said, cx);
    let shown = rendered(&thread, "u", cx).expect("its words have a text view");
    for words in [
        "Plan",
        "Read main.rs, then check it:",
        "one",
        "docs",
        "quoted",
    ] {
        assert!(shown.contains(words), "{words:?} in {shown:?}");
    }
    for mark in ["## ", "**", "`", "- ", "](", "> "] {
        assert!(
            !shown.contains(mark),
            "{mark:?} is markdown, not words: {shown:?}"
        );
    }
    assert!(
        rendered(&thread, "a", cx).is_some(),
        "an answer's are drawn the same way"
    );
}

#[gpui_kit::test]
fn a_long_message_is_cut_to_eight_lines_until_it_is_opened(cx: &mut TestAppContext) {
    let said = (1..=20).fold(String::new(), |said, line| {
        said + &format!("- line {line}\n")
    });
    let (window, view, thread) = saying("chat-said-long", &said, cx);
    let height = |cx: &mut TestAppContext| {
        let row = VisualTestContext::from_window(window, cx).debug_bounds("chat-row-0");
        row.unwrap().size.height
    };
    let clamped = |cx: &TestAppContext| {
        thread.read_with(cx, |thread, cx| {
            let text = thread.drawn_text(&ItemId("u".into())).unwrap();
            text.read(cx).is_clamped()
        })
    };
    let closed = height(cx);
    assert!(clamped(cx), "cut short");

    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let id = timeline.read_with(cx, |timeline, _| timeline.rows[0].id.clone());
    timeline.update(cx, |timeline, cx| timeline.toggle(id.clone(), 0, cx));
    draw(window, cx);
    assert!(
        height(cx) > closed * 1.5,
        "all of it: {closed:?} to {:?}",
        height(cx)
    );

    timeline.update(cx, |timeline, cx| timeline.toggle(id, 0, cx));
    draw(window, cx);
    assert!(clamped(cx));
    assert_eq!(height(cx), closed);
}

#[test]
fn a_picture_shows_and_opens_as_one_and_any_other_file_goes_where_the_person_would() {
    let attached = |mime: &str| Attachment {
        file: "f".into(),
        name: "f".into(),
        mime: mime.into(),
    };
    assert!(super::super::attachments::shows_picture(&attached(
        "image/png"
    )));
    assert!(!super::super::attachments::shows_picture(&attached(
        "application/octet-stream"
    )));
}

#[gpui_kit::test]
fn a_picture_clicked_opens_in_the_viewer_over_the_chat_until_escape(cx: &mut TestAppContext) {
    let (window, view, _) = saying("chat-said-picture", "Look", cx);
    let folder = scratch("chat-said-picture-file");
    let path = folder.join("picture.png");
    image::RgbImage::new(8, 8).save(&path).unwrap();
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    timeline.update(cx, |_, cx| cx.emit(OpenPicture(path.clone())));
    draw(window, cx);

    let over = |cx: &mut TestAppContext| {
        VisualTestContext::from_window(window, cx).debug_bounds("chat-picture")
    };
    assert!(over(cx).is_some(), "the viewer is over the chat");
    let viewer = view.read_with(cx, |view, _| view.picture.clone()).unwrap();
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.path().to_owned()),
        path
    );
    let focused = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            let viewer = viewer.read(cx).focus_handle(cx);
            viewer.is_focused(window)
        })
        .unwrap()
    };
    assert!(focused(cx), "it has the keys");

    cx.simulate_keystrokes(window, "escape");
    draw(window, cx);
    assert!(over(cx).is_none());
    assert!(view.read_with(cx, |view, _| view.picture.is_none()));
    let composer = cx
        .update_window(window, |_, window, cx| {
            view.read(cx)
                .composer
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        })
        .unwrap();
    assert!(composer, "the keys are back with the composer");
}

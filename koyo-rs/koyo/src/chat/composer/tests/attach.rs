//! What's attached to the message being written: its tiles, its files, and what becomes of them.

use std::path::{Path, PathBuf};

use gpui_kit::{
    AppContext as _, ClipboardEntry, ClipboardItem, ExternalPaths, FileDropEvent, Image,
    ImageFormat, Pixels, PlatformInput, Point, Size, TestAppContext, px,
};
use koyo_agent::protocol::model::ToolFamily;
use koyo_agent::protocol::requests::{Approval, Request, RequestPayload, RequestStatus};

use super::{catalog, catalog_is, chat, type_in};
use crate::chat::attachments::Source;
use crate::chat::composer::attach::Attached;
use crate::chat::tests::asks::{Asking, request};
use crate::test_support::draw;

/// A chat that keeps what's attached in a folder of the test's own.
fn attaching(name: &str, cx: &mut TestAppContext) -> (Asking, PathBuf) {
    let chat = chat(name, cx);
    let folder = chat.composer.update(cx, |composer, _| {
        let folder = composer.cwd.join("attachments");
        composer.attachments = Some(folder.clone());
        folder
    });
    (chat, folder)
}

fn picture() -> Vec<u8> {
    let mut bytes = Vec::new();
    let picture = image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 4));
    picture
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

fn attach_picture(chat: &Asking, cx: &mut TestAppContext) {
    let source = Source::Picture(picture());
    chat.composer
        .update(cx, |composer, cx| composer.attach(vec![source], cx));
}

fn tiles(chat: &Asking, cx: &TestAppContext) -> usize {
    chat.composer
        .read_with(cx, |composer, _| composer.attached.len())
}

pub(super) fn kept_files(folder: &Path) -> usize {
    std::fs::read_dir(folder).map_or(0, Iterator::count)
}

fn send(chat: &Asking, cx: &mut TestAppContext) {
    let composer = chat.composer.clone();
    cx.update_window(chat.window, |_, window, cx| {
        composer.update(cx, |composer, cx| composer.send(window, cx));
    })
    .unwrap();
    draw(chat.window, cx);
}

fn can_send(chat: &Asking, cx: &TestAppContext) -> bool {
    chat.composer.read_with(cx, |composer, cx| {
        composer.can_send(&composer.text.read(cx).value())
    })
}

#[gpui_kit::test]
fn a_picture_shows_as_it_is_kept_and_a_message_waits_for_it(cx: &mut TestAppContext) {
    let (chat, folder) = attaching("chat-attach-waits", cx);
    type_in(&chat, "Look", cx);

    attach_picture(&chat, cx);

    assert_eq!(tiles(&chat, cx), 1, "its tile is there at once");
    chat.composer
        .update(cx, |composer, _| assert!(composer.attaching()));
    assert!(!can_send(&chat, cx), "a message waits for what it attaches");

    draw(chat.window, cx);

    assert!(can_send(&chat, cx));
    assert_eq!(kept_files(&folder), 1);
}

#[gpui_kit::test]
fn a_picture_alone_can_be_sent_and_comes_back_if_it_does_not_go(cx: &mut TestAppContext) {
    let (chat, folder) = attaching("chat-attach-back", cx);
    attach_picture(&chat, cx);
    draw(chat.window, cx);

    send(&chat, cx);

    // No host hears it here, so it didn't go.
    assert_eq!(tiles(&chat, cx), 1);
    assert_eq!(kept_files(&folder), 1, "its file stays for when it does");
    let why = chat
        .composer
        .read_with(cx, |composer, _| composer.error.clone());
    assert!(why.is_some());
}

#[gpui_kit::test]
fn taking_a_tile_away_takes_its_file(cx: &mut TestAppContext) {
    let (chat, folder) = attaching("chat-attach-detach", cx);
    attach_picture(&chat, cx);
    draw(chat.window, cx);
    assert_eq!(kept_files(&folder), 1);

    chat.composer.update(cx, |composer, cx| {
        let id = composer.attached.first().map(Attached::id).unwrap();
        composer.detach(id, cx);
    });
    draw(chat.window, cx);

    assert_eq!(tiles(&chat, cx), 0);
    assert_eq!(kept_files(&folder), 0);
}

#[gpui_kit::test]
fn what_can_not_be_attached_says_why_and_leaves_no_tile(cx: &mut TestAppContext) {
    let (chat, _) = attaching("chat-attach-refused", cx);
    let gone = Source::File("/nowhere/at-all.txt".into());

    chat.composer
        .update(cx, |composer, cx| composer.attach(vec![gone], cx));
    draw(chat.window, cx);

    assert_eq!(tiles(&chat, cx), 0);
    let why = chat
        .composer
        .read_with(cx, |composer, _| composer.error.clone());
    assert!(why.is_some_and(|why| why.starts_with("Can't attach at-all.txt: ")));
}

/// A file let go at the place `at` gives, for the window's size.
fn let_go(chat: &Asking, at: impl FnOnce(Size<Pixels>) -> Point<Pixels>, cx: &mut TestAppContext) {
    let cwd = chat
        .composer
        .read_with(cx, |composer, _| composer.cwd.clone());
    let notes = cwd.join("notes.md");
    std::fs::write(&notes, "# Notes\n").unwrap();
    cx.update_window(chat.window, |_, window, cx| {
        let position = at(window.viewport_size());
        let paths = ExternalPaths(vec![notes].into());
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Entered { position, paths }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Submit { position }),
            cx,
        );
    })
    .unwrap();
    draw(chat.window, cx);
}

#[gpui_kit::test]
fn files_let_go_over_the_timeline_are_attached(cx: &mut TestAppContext) {
    let (chat, folder) = attaching("chat-attach-drop", cx);

    let_go(
        &chat,
        |size| gpui_kit::point(size.width * 0.75, size.height * 0.5),
        cx,
    );

    assert_eq!(tiles(&chat, cx), 1);
    assert_eq!(kept_files(&folder), 1);
}

#[gpui_kit::test]
fn files_let_go_over_the_glass_are_attached(cx: &mut TestAppContext) {
    let (chat, folder) = attaching("chat-attach-drop-glass", cx);

    // In the middle of the composer floating at the bottom of the tab, what's on the glass blocking
    // the pointer from everything behind it.
    let_go(
        &chat,
        |size| gpui_kit::point(size.width * 0.6, size.height - px(85.)),
        cx,
    );

    assert_eq!(tiles(&chat, cx), 1);
    assert_eq!(kept_files(&folder), 1);
}

/// A chat that waits on an approval, which keeps what's attached in a folder of the test's own.
fn asked_to_approve(name: &str, cx: &mut TestAppContext) -> Asking {
    let approval = RequestPayload::Approval(Approval {
        tool: "Bash".into(),
        family: ToolFamily::Command,
        title: "Run".into(),
        label: "ls".into(),
        detail: "ls".into(),
        change: None,
        input: serde_json::Value::Null,
        reason: None,
        allow_session: None,
    });
    let pending = Request {
        payload: approval,
        ..request("a1", RequestStatus::Pending, true, 10)
    };
    let chat = Asking::new(name, vec![pending], cx);
    catalog_is(&chat, catalog(), cx);
    let cwd = chat
        .composer
        .read_with(cx, |composer, _| composer.cwd.clone());
    chat.composer.update(cx, |composer, _| {
        composer.attachments = Some(cwd.join("attachments"));
    });
    chat
}

#[gpui_kit::test]
fn what_is_typed_to_decline_leaves_what_is_attached_for_the_next_message(cx: &mut TestAppContext) {
    let chat = asked_to_approve("chat-attach-decline", cx);
    attach_picture(&chat, cx);
    draw(chat.window, cx);
    type_in(&chat, "No", cx);

    send(&chat, cx);

    assert_eq!(chat.said(cx).len(), 1);
    assert_eq!(tiles(&chat, cx), 1);
}

#[gpui_kit::test]
fn what_is_typed_to_decline_does_not_wait_for_what_is_being_attached(cx: &mut TestAppContext) {
    let chat = asked_to_approve("chat-attach-decline-waits", cx);
    type_in(&chat, "No", cx);
    attach_picture(&chat, cx);

    chat.composer
        .read_with(cx, |composer, _| assert!(composer.attaching()));
    assert!(can_send(&chat, cx));
}

#[gpui_kit::test]
fn a_message_sent_again_as_it_was_keeps_its_id(cx: &mut TestAppContext) {
    let (chat, _) = attaching("chat-attach-retry", cx);
    let unsent = |chat: &Asking, cx: &TestAppContext| {
        chat.composer.read_with(cx, |composer, _| {
            composer
                .unsent
                .as_ref()
                .map(|prompt| prompt.message_id.clone())
        })
    };
    attach_picture(&chat, cx);
    draw(chat.window, cx);
    type_in(&chat, "Look", cx);

    // No host hears it here, so each of these comes back.
    send(&chat, cx);
    let first = unsent(&chat, cx);
    assert!(first.is_some());
    send(&chat, cx);
    assert_eq!(unsent(&chat, cx), first, "nothing changed");

    type_in(&chat, " closer", cx);
    send(&chat, cx);
    assert_ne!(unsent(&chat, cx), first, "it's another message now");
}

#[gpui_kit::test]
fn a_pasted_picture_is_attached_and_pasted_words_are_typed(cx: &mut TestAppContext) {
    let (chat, folder) = attaching("chat-attach-paste", cx);

    cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::Image(Image::from_bytes(
            ImageFormat::Png,
            picture(),
        ))],
    });
    chat.keys("cmd-v", cx);
    draw(chat.window, cx);

    assert_eq!(tiles(&chat, cx), 1);
    assert_eq!(kept_files(&folder), 1);
    assert_eq!(chat.typed(cx), "");

    cx.write_to_clipboard(ClipboardItem::new_string("words".to_owned()));
    chat.keys("cmd-v", cx);
    draw(chat.window, cx);

    assert_eq!(tiles(&chat, cx), 1);
    assert_eq!(chat.typed(cx), "words");
}

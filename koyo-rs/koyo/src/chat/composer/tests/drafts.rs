//! A chat's draft: there again when the chat next opens, after a restart too, and only ever in one
//! composer.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, Entity, TestAppContext, Window, px, size,
};
use koyo_agent::agent_client::Notice;
use koyo_agent::protocol::events::{Event, Removed};
use koyo_agent::protocol::ids::{MessageId, ThreadId};
use koyo_agent::protocol::model::{Attachment, Prompt};

use super::super::ChatComposer;
use super::attach::kept_files;
use crate::chat::Chats;
use crate::chat::drafts::{self, DraftOf};
use crate::test_support::{in_window, scratch, write};

struct Place {
    chats: Entity<Chats>,
    cwd: PathBuf,
    id: ThreadId,
    /// Where what's attached is kept.
    files: PathBuf,
    /// A file to attach.
    note: PathBuf,
}

impl Place {
    fn new(name: &str, cx: &mut TestAppContext) -> Place {
        cx.update(crate::init);
        let (cwd, files) = (scratch(name), scratch(&format!("{name}-files")));
        cx.update(|cx| drafts::attach_into(&files, cx));
        write(&cwd, "note.txt", b"a note");
        Place {
            chats: cx.new(Chats::new),
            note: cwd.join("note.txt"),
            cwd,
            id: ThreadId("0001".into()),
            files,
        }
    }

    fn thread(&self) -> DraftOf {
        DraftOf::Thread(self.id.clone())
    }

    fn new_chat(&self) -> DraftOf {
        DraftOf::NewChat(self.cwd.clone())
    }
}

struct Open {
    window: AnyWindowHandle,
    composer: Entity<ChatComposer>,
}

impl Open {
    fn thread(place: &Place, cx: &mut TestAppContext) -> Open {
        Open::on(place, Some(&place.id), cx)
    }

    fn new_chat(place: &Place, cx: &mut TestAppContext) -> Open {
        Open::on(place, None, cx)
    }

    fn on(place: &Place, thread: Option<&ThreadId>, cx: &mut TestAppContext) -> Open {
        let (window, composer) = in_window(size(px(800.), px(400.)), cx, |window, cx| {
            let chats = place.chats.clone();
            let opened = thread.map(|id| chats.update(cx, |chats, cx| chats.thread(id, cx)));
            let catalog = chats.update(cx, |chats, cx| chats.catalog(&place.cwd, cx));
            let cwd = place.cwd.clone();
            cx.new(|cx| ChatComposer::new(chats, catalog, cwd, "here".into(), opened, window, cx))
        });
        cx.run_until_parked();
        Open { window, composer }
    }

    fn with(
        &self,
        cx: &mut TestAppContext,
        then: impl FnOnce(&mut ChatComposer, &mut Window, &mut Context<ChatComposer>),
    ) {
        cx.update_window(self.window, |_, window, cx| {
            self.composer
                .update(cx, |composer, cx| then(composer, window, cx));
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn write(&self, text: &str, cx: &mut TestAppContext) {
        self.with(cx, |composer, window, cx| {
            composer
                .text
                .update(cx, |typed, cx| typed.set_value(text.to_owned(), window, cx));
        });
    }

    fn attach(&self, path: &PathBuf, cx: &mut TestAppContext) {
        self.with(cx, |composer, _, cx| {
            composer.attach_paths(std::slice::from_ref(path), cx);
        });
    }

    fn typed(&self, cx: &TestAppContext) -> String {
        self.composer.read_with(cx, |composer, cx| {
            composer.text.read(cx).value().to_string()
        })
    }

    fn tiles(&self, cx: &TestAppContext) -> usize {
        self.composer
            .read_with(cx, |composer, _| composer.attached.len())
    }

    /// Its window goes, and it with that.
    fn close(self, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, _| window.remove_window())
            .unwrap();
        drop(self.composer);
        // What's dropped is released as the next update ends.
        cx.update(|_| {});
        cx.run_until_parked();
    }
}

fn kept_text(of: &DraftOf, cx: &TestAppContext) -> Option<String> {
    cx.read(|cx| drafts::kept(of, cx)).map(|draft| draft.text)
}

#[gpui_kit::test]
fn a_chat_opened_again_has_what_was_being_written_with_the_caret_where_it_was(
    cx: &mut TestAppContext,
) {
    let place = Place::new("draft-thread", cx);
    let open = Open::thread(&place, cx);
    open.write("hello world", cx);
    open.with(cx, |composer, _, cx| {
        composer
            .text
            .update(cx, |typed, cx| typed.set_selected_range(5..5, cx));
    });
    open.close(cx);

    let again = Open::thread(&place, cx);

    assert_eq!(again.typed(cx), "hello world");
    assert_eq!(
        again
            .composer
            .read_with(cx, |composer, cx| composer.text.read(cx).cursor()),
        5
    );
}

#[gpui_kit::test]
fn a_new_chat_has_the_draft_of_its_folder_and_no_other(cx: &mut TestAppContext) {
    let place = Place::new("draft-new", cx);
    let open = Open::new_chat(&place, cx);
    open.write("a thought", cx);
    open.close(cx);

    let again = Open::new_chat(&place, cx);
    assert_eq!(again.typed(cx), "a thought");

    assert_eq!(Open::thread(&place, cx).typed(cx), "");
    let elsewhere = Place {
        cwd: scratch("draft-new-elsewhere"),
        ..place
    };
    assert_eq!(Open::new_chat(&elsewhere, cx).typed(cx), "");
}

#[gpui_kit::test]
fn nothing_written_is_no_draft(cx: &mut TestAppContext) {
    let place = Place::new("draft-empty", cx);
    let open = Open::thread(&place, cx);
    open.write("hello", cx);
    assert_eq!(kept_text(&place.thread(), cx).as_deref(), Some("hello"));

    open.write("", cx);

    assert_eq!(kept_text(&place.thread(), cx), None);
}

#[gpui_kit::test]
fn a_message_that_does_not_go_is_a_draft_again(cx: &mut TestAppContext) {
    let place = Place::new("draft-unsent", cx);
    let open = Open::thread(&place, cx);
    open.write("hello", cx);

    // These chats have no host, so it can't go.
    open.with(cx, ChatComposer::send);

    assert_eq!(open.typed(cx), "hello");
    assert_eq!(kept_text(&place.thread(), cx).as_deref(), Some("hello"));
}

#[gpui_kit::test]
fn what_is_written_while_a_new_chat_is_made_goes_to_its_thread(cx: &mut TestAppContext) {
    let place = Place::new("draft-created", cx);
    let open = Open::new_chat(&place, cx);
    open.write("and another thing", cx);
    assert_eq!(
        kept_text(&place.new_chat(), cx).as_deref(),
        Some("and another thing")
    );

    open.with(cx, |composer, _, cx| composer.draft_goes_to(&place.id, cx));

    assert_eq!(kept_text(&place.new_chat(), cx), None);
    assert_eq!(
        kept_text(&place.thread(), cx).as_deref(),
        Some("and another thing")
    );
    assert_eq!(Open::new_chat(&place, cx).typed(cx), "");
}

#[gpui_kit::test]
fn a_second_composer_on_a_chat_starts_empty_and_keeps_nothing(cx: &mut TestAppContext) {
    let place = Place::new("draft-second", cx);
    let first = Open::thread(&place, cx);
    first.write("mine", cx);

    let second = Open::thread(&place, cx);
    assert_eq!(second.typed(cx), "");
    second.write("theirs", cx);
    assert_eq!(kept_text(&place.thread(), cx).as_deref(), Some("mine"));
    second.close(cx);

    // Its going leaves the first one's alone.
    first.write("still mine", cx);
    assert_eq!(
        kept_text(&place.thread(), cx).as_deref(),
        Some("still mine")
    );
}

#[gpui_kit::test]
fn what_a_draft_attaches_outlives_its_tab_and_goes_with_its_tile(cx: &mut TestAppContext) {
    let place = Place::new("draft-files", cx);
    let open = Open::thread(&place, cx);
    open.attach(&place.note, cx);
    open.close(cx);
    assert_eq!(kept_files(&place.files), 1, "its draft still names it");

    let again = Open::thread(&place, cx);
    assert_eq!(again.tiles(cx), 1);
    let tile = again
        .composer
        .read_with(cx, |composer, _| composer.attached[0].id());
    again.with(cx, |composer, _, cx| composer.detach(tile, cx));

    assert_eq!(kept_files(&place.files), 0);
    assert_eq!(cx.read(|cx| drafts::kept(&place.thread(), cx)), None);
}

#[gpui_kit::test]
fn a_composer_with_no_draft_lets_go_of_what_it_attached(cx: &mut TestAppContext) {
    let place = Place::new("draft-no-draft", cx);
    let _first = Open::thread(&place, cx);
    let second = Open::thread(&place, cx);
    second.attach(&place.note, cx);
    assert_eq!(kept_files(&place.files), 1);

    second.close(cx);

    assert_eq!(kept_files(&place.files), 0);
}

#[gpui_kit::test]
fn a_file_that_went_is_not_brought_back(cx: &mut TestAppContext) {
    let place = Place::new("draft-file-gone", cx);
    let open = Open::thread(&place, cx);
    open.attach(&place.note, cx);
    open.close(cx);
    std::fs::remove_dir_all(&place.files).unwrap();

    assert_eq!(Open::thread(&place, cx).tiles(cx), 0);
}

#[gpui_kit::test]
fn a_deleted_thread_takes_its_draft_and_its_files_with_it(cx: &mut TestAppContext) {
    let place = Place::new("draft-removed", cx);
    let open = Open::thread(&place, cx);
    open.write("hello", cx);
    open.attach(&place.note, cx);
    assert_eq!(kept_files(&place.files), 1);

    place.chats.update(cx, |chats, cx| {
        let removed = Event::ThreadRemoved(Removed {
            thread_id: place.id.clone(),
        });
        chats.take_in(vec![Notice::Event(Box::new(removed))], cx);
    });
    cx.run_until_parked();

    assert_eq!(kept_text(&place.thread(), cx), None);
    assert_eq!(kept_files(&place.files), 0);
    // Its tab is on its way, and may hear something before it goes.
    open.write("hello again", cx);
    assert_eq!(kept_text(&place.thread(), cx), None);
}

#[gpui_kit::test]
fn a_draft_is_saved_a_moment_after_the_last_change_for_the_next_start(cx: &mut TestAppContext) {
    let place = Place::new("draft-saved", cx);
    let path = scratch("draft-saved-file").join("chat-drafts.json");
    cx.update(|cx| drafts::open_at(&path, cx));
    let open = Open::thread(&place, cx);
    open.write("hello", cx);

    cx.executor().advance_clock(Duration::from_millis(299));
    cx.run_until_parked();
    assert!(!path.exists());
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert!(path.exists());
    open.close(cx);

    cx.update(|cx| drafts::open_at(&path, cx));
    assert_eq!(Open::thread(&place, cx).typed(cx), "hello");
}

#[gpui_kit::test]
fn a_change_still_settling_is_saved_when_the_app_quits(cx: &mut TestAppContext) {
    let place = Place::new("draft-quit", cx);
    let path = scratch("draft-quit-file").join("chat-drafts.json");
    cx.update(|cx| drafts::open_at(&path, cx));
    let open = Open::thread(&place, cx);
    open.write("hello", cx);
    assert!(!path.exists());

    cx.quit();

    assert!(std::fs::read_to_string(&path).unwrap().contains("hello"));
}

#[gpui_kit::test]
fn a_file_that_cannot_be_read_is_no_drafts(cx: &mut TestAppContext) {
    let place = Place::new("draft-corrupt", cx);
    let path = scratch("draft-corrupt-file").join("chat-drafts.json");
    std::fs::write(&path, "{ not json").unwrap();
    cx.update(|cx| drafts::open_at(&path, cx));

    assert_eq!(Open::thread(&place, cx).typed(cx), "");
}

#[gpui_kit::test]
fn words_taken_back_go_after_whats_typed_with_the_files_that_are_still_there(
    cx: &mut TestAppContext,
) {
    let place = Place::new("draft-take-back", cx);
    let open = Open::thread(&place, cx);
    open.attach(&place.note, cx);
    let kept: Vec<Attachment> = open.composer.read_with(cx, |composer, _| {
        composer.kept_attachments().cloned().collect()
    });
    open.with(cx, |composer, _, _| composer.attached.clear());
    let gone = Attachment {
        file: "gone.txt".into(),
        ..kept[0].clone()
    };
    open.write("typed", cx);

    open.with(cx, |composer, window, cx| {
        let prompt = Prompt {
            message_id: MessageId("m".into()),
            text: "said".into(),
            attachments: vec![gone, kept[0].clone()],
        };
        composer.take_back(prompt, window, cx);
    });

    assert_eq!(open.typed(cx), "typed\n\nsaid");
    assert_eq!(open.tiles(cx), 1, "what's gone would only stop it going");
    let caret = open
        .composer
        .read_with(cx, |composer, cx| composer.text.read(cx).cursor());
    assert_eq!(caret, "typed\n\nsaid".len());
}

//! What a person sets a chat to from the composer's footer: what the host is asked to change, and
//! nothing else, by the keys and the menus. No host hears it here, so what the composer asked is
//! what's looked at; what shows is what the thread says, as it does once the host answers.

use gpui_kit::{AppContext as _, KeyUpEvent, Keystroke, PlatformInput, TestAppContext, px};
use koyo_agent::protocol::events::Changed;
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{Catalog, Model, UpdateParams};
use koyo_agent::protocol::model::{Effort, Mode, Thread};
use koyo_agent::protocol::requests::{Questions, RequestPayload, RequestStatus};

use super::footer::{ROOM_FOR_LABELS, SetPermission, labels_fitting};
use crate::chat::chosen::Settings;
use crate::chat::tests::asks::{Asking, request};
use crate::chat::tests::{question, snapshot};
use crate::chat::thread::ThreadEvent;
use crate::test_support::draw;

mod drafts;

mod attach;
mod effort;
mod markdown;
mod missing;
mod suggest;

fn model(id: &str, efforts: &[Effort], fast: bool, auto: bool) -> Model {
    Model {
        id: id.into(),
        runs: None,
        name: format!("{id} (latest)"),
        label: format!("{id} (latest)"),
        efforts: efforts.to_vec(),
        fast,
        auto,
    }
}

fn catalog() -> Catalog {
    let efforts = [Effort::Low, Effort::High, Effort::Max];
    Catalog {
        models: vec![
            model("first", &efforts, true, true),
            model("second", &[Effort::Low, Effort::High], false, true),
            model("third", &[], false, false),
        ],
        commands: Vec::new(),
        default_mode: Some(Mode::Auto),
        default_model: None,
    }
}

/// The host now says the chat can choose from `to`.
fn catalog_is(chat: &Asking, to: Catalog, cx: &mut TestAppContext) {
    let held = chat
        .composer
        .read_with(cx, |composer, _| composer.catalog.clone());
    held.update(cx, |held, cx| {
        *held = to;
        cx.notify();
    });
    draw(chat.window, cx);
}

/// A chat whose host says what it can choose from.
fn chat(name: &str, cx: &mut TestAppContext) -> Asking {
    let chat = Asking::new(name, Vec::new(), cx);
    catalog_is(&chat, catalog(), cx);
    chat
}

fn nothing() -> UpdateParams {
    UpdateParams::of(ThreadId("0001".into()))
}

fn updated(chat: &Asking, cx: &TestAppContext) -> Vec<UpdateParams> {
    chat.composer
        .read_with(cx, |composer, _| composer.updated.clone())
}

/// Presses `keys` one by one, each let go as a person would: an arrow not let go is held.
fn press(chat: &Asking, keys: &'static str, cx: &mut TestAppContext) {
    for key in keys.split(' ') {
        chat.keys(key, cx);
        let keystroke = Keystroke::parse(key).unwrap();
        cx.update_window(chat.window, |_, window, cx| {
            window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
        })
        .unwrap();
    }
}

/// The host says the thread is now set as `set` makes it, and the composer shows it.
fn set_to(chat: &Asking, set: impl FnOnce(&mut Thread), seq: u64, cx: &mut TestAppContext) {
    let mut thread = snapshot("0001", seq, Vec::new()).thread;
    set(&mut thread);
    let event = ThreadEvent::Changed(Box::new(Changed {
        thread_id: ThreadId("0001".into()),
        seq,
        thread: Some(thread),
        runs: Vec::new(),
        items: Vec::new(),
        requests: Vec::new(),
    }));
    chat.thread
        .update(cx, |thread, cx| thread.take_in(vec![event], cx));
    draw(chat.window, cx);
}

fn settings(chat: &Asking, cx: &TestAppContext) -> Settings {
    chat.composer
        .read_with(cx, |composer, _| composer.settings.clone())
}

fn picking(chat: &Asking, cx: &TestAppContext) -> bool {
    chat.composer
        .read_with(cx, |composer, cx| composer.model_menu.is_open(cx))
}

#[gpui_kit::test]
fn the_key_for_the_plan_asks_for_that_and_shows_what_the_thread_then_says(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-plan", cx);

    chat.keys("shift-tab", cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            plan: Some(true),
            ..nothing()
        }]
    );
    assert!(!settings(&chat, cx).plan, "it isn't so until the host says");
    set_to(&chat, |thread| thread.plan = true, 11, cx);
    assert!(settings(&chat, cx).plan);
}

#[gpui_kit::test]
fn the_keys_of_the_menu_pick_a_model(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-menu", cx);

    press(&chat, "ctrl-m down enter", cx);
    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            model: Some(Some("second".into())),
            ..nothing()
        }],
    );
    assert!(!picking(&chat, cx), "picking closes it");
    assert_eq!(chat.typed(cx), "", "and none of it went into what's typed");
}

#[gpui_kit::test]
fn a_chat_that_chose_no_model_shows_and_opens_on_the_one_the_agent_runs(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-default-model", cx);
    let shown = |cx: &TestAppContext| {
        (chat.composer).read_with(cx, |composer, cx| {
            composer.model_label(composer.catalog.read(cx))
        })
    };
    assert_eq!(
        shown(cx),
        "first",
        "with no setting the first, recommended, runs"
    );
    let settled = Catalog {
        default_model: Some("second".into()),
        ..catalog()
    };
    catalog_is(&chat, settled, cx);
    assert_eq!(shown(cx), "second", "the agent's settings say");

    press(&chat, "ctrl-m enter", cx);
    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            model: Some(Some("second".into())),
            ..nothing()
        }],
        "the menu opens on it",
    );
}

#[gpui_kit::test]
fn the_keys_stop_at_the_ends(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-ends", cx);

    press(&chat, "ctrl-m up up down down enter", cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            model: Some(Some("third".into())),
            permission: Some(Some(Mode::Ask)),
            ..nothing()
        }],
        "it can't run in the Auto the person's settings chose, so it asks",
    );
}

#[gpui_kit::test]
fn escape_closes_the_menu_and_the_keys_go_back_to_what_is_typed(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-escape", cx);

    press(&chat, "a ctrl-m", cx);
    assert!(picking(&chat, cx));
    chat.keys("escape", cx);

    assert!(!picking(&chat, cx));
    assert_eq!(updated(&chat, cx), []);
    chat.keys("b", cx);
    assert_eq!(chat.typed(cx), "ab", "they're the input's again");
    chat.keys("enter", cx);
    let models = updated(&chat, cx)
        .into_iter()
        .filter(|asked| asked.model.is_some())
        .count();
    assert_eq!(models, 0, "and picking is over");
}

#[gpui_kit::test]
fn the_key_closes_the_menu_it_opened(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-toggle", cx);

    chat.keys("ctrl-m", cx);
    assert!(picking(&chat, cx));
    chat.keys("ctrl-m", cx);

    assert!(!picking(&chat, cx));
}

#[gpui_kit::test]
fn a_model_keeps_only_what_it_can_do(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-keeps", cx);
    let set = |thread: &mut Thread| {
        thread.model = Some("first".into());
        thread.effort = Some(Effort::Max);
        thread.fast = true;
        thread.permission = Some(Mode::Auto);
    };
    set_to(&chat, set, 11, cx);

    press(&chat, "ctrl-m down down enter", cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            model: Some(Some("third".into())),
            effort: Some(None),
            fast: Some(false),
            permission: Some(Some(Mode::Ask)),
            ..nothing()
        }],
    );
}

#[gpui_kit::test]
fn fast_is_the_last_row_if_the_model_of_the_chat_can_go_fast(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-fast", cx);

    press(&chat, "ctrl-m down down down enter", cx);
    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            fast: Some(true),
            ..nothing()
        }]
    );

    press(&chat, "escape", cx);
    set_to(&chat, |thread| thread.model = Some("second".into()), 11, cx);
    press(&chat, "ctrl-m down down down enter", cx);
    assert_eq!(
        updated(&chat, cx).last(),
        Some(&UpdateParams {
            model: Some(Some("third".into())),
            permission: Some(Some(Mode::Ask)),
            ..nothing()
        }),
        "the second model can't, so the last row is the last model",
    );
}

#[gpui_kit::test]
fn how_much_the_agent_may_do_is_all_that_is_asked_for(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-permission", cx);

    chat.says(&SetPermission(Mode::FullAccess), cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            permission: Some(Some(Mode::FullAccess)),
            ..nothing()
        }],
    );
}

#[gpui_kit::test]
fn what_it_is_set_to_already_is_not_asked_for(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-same", cx);
    set_to(&chat, |thread| thread.permission = Some(Mode::Ask), 11, cx);

    chat.says(&SetPermission(Mode::Ask), cx);

    assert_eq!(updated(&chat, cx), []);
}

#[gpui_kit::test]
fn compacting_says_why_it_did_not_happen(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-compact", cx);

    chat.composer.update(cx, super::ChatComposer::compact);
    cx.run_until_parked();

    let error = chat
        .composer
        .read_with(cx, |composer, _| composer.error.clone());
    assert!(error.is_some(), "no host heard it");
}

#[gpui_kit::test]
fn the_ring_opens_how_full_it_is_and_closes_it(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-context", cx);
    let press = |cx: &mut TestAppContext| {
        cx.update_window(chat.window, |_, window, cx| {
            chat.composer
                .update(cx, |composer, cx| composer.toggle_context(window, cx));
        })
        .unwrap();
        chat.composer
            .read_with(cx, |composer, cx| composer.context_menu.is_open(cx))
    };
    assert!(!press(cx), "no run has said yet");

    chat.composer
        .update(cx, |composer, _| composer.context = Some((86, 967)));

    assert!(press(cx));
    assert!(!press(cx));
}

#[gpui_kit::test]
fn a_new_catalog_closes_the_menu_opened_on_the_old(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-new-catalog", cx);
    press(&chat, "ctrl-m down down", cx);

    catalog_is(&chat, Catalog::default(), cx);

    assert!(!picking(&chat, cx));
}

#[gpui_kit::test]
fn the_menu_has_the_keys_ahead_of_what_is_asked(cx: &mut TestAppContext) {
    let asked = RequestPayload::Question(Questions {
        questions: vec![question("Tabs?", false, &["Tabs", "Spaces"])],
        live: true,
    });
    let pending = koyo_agent::protocol::requests::Request {
        payload: asked,
        ..request("q1", RequestStatus::Pending, false, 10)
    };
    let chat = Asking::new("chat-footer-asked", vec![pending], cx);
    catalog_is(&chat, catalog(), cx);

    press(&chat, "ctrl-m down enter", cx);

    assert_eq!(chat.said(cx), [], "it's what's asked that waits");
    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            model: Some(Some("second".into())),
            ..nothing()
        }],
    );
    chat.keys("1", cx);
    assert_eq!(
        chat.typed(cx),
        "",
        "and once it's closed a digit chooses again"
    );
}

#[gpui_kit::test]
fn a_page_of_rows_stops_at_the_end(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-page", cx);

    press(&chat, "ctrl-m pagedown enter", cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            fast: Some(true),
            ..nothing()
        }],
        "Fast is the last"
    );
}

#[gpui_kit::test]
fn fast_flips_where_it_is_and_the_menu_stays_up(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-fast-switch", cx);

    press(&chat, "ctrl-m pagedown enter", cx);
    assert!(picking(&chat, cx), "it's a switch, not a pick");

    press(&chat, "escape", cx);
    assert!(!picking(&chat, cx));
}

#[gpui_kit::test]
fn a_new_chat_holds_what_it_will_start_with(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-new", cx);
    chat.composer
        .update(cx, |composer, _| composer.thread = None);

    press(&chat, "ctrl-m down enter shift-tab", cx);
    type_in(&chat, "/eff", cx);
    chat.keys("enter", cx);
    press(&chat, "left escape", cx);

    assert_eq!(updated(&chat, cx), [], "there's no thread to ask");
    let set = settings(&chat, cx);
    let made = set.to_create(std::path::Path::new("/work"));
    assert_eq!(made.model.as_deref(), Some("second"));
    assert_eq!(made.effort, Some(Effort::Low), "from the High it showed");
    assert!(made.plan);
}

#[test]
fn the_chips_say_less_from_the_end_as_the_composer_narrows() {
    assert_eq!(labels_fitting(px(760.)), ROOM_FOR_LABELS.len());
    assert_eq!(labels_fitting(px(480.)), 3);
    assert_eq!(labels_fitting(px(420.)), 2);
    assert_eq!(labels_fitting(px(400.)), 1);
    assert_eq!(labels_fitting(px(300.)), 0);
}

/// `typed` goes in where the caret is, as if typed.
fn type_in(chat: &Asking, typed: &str, cx: &mut TestAppContext) {
    let (composer, typed) = (chat.composer.clone(), typed.to_owned());
    cx.update_window(chat.window, |_, window, cx| {
        composer
            .read(cx)
            .text
            .clone()
            .update(cx, |text, cx| text.insert(typed, window, cx));
    })
    .unwrap();
    draw(chat.window, cx);
}

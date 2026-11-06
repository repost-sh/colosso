//! How hard the agent thinks: the effort a chat keeps or takes on another model, the slider that
//! moves it, and the effort a message goes with.

use gpui_kit::TestAppContext;
use koyo_agent::protocol::methods::UpdateParams;
use koyo_agent::protocol::model::Effort;

use super::{chat, nothing, picking, press, set_to, type_in, updated};
use crate::chat::tests::asks::Asking;
use crate::test_support::draw;

fn sliding(chat: &Asking, cx: &TestAppContext) -> bool {
    chat.composer
        .read_with(cx, |composer, cx| composer.effort_menu.is_open(cx))
}

/// A thread on `model` at `effort`, with the slider open on it.
fn slider_on(name: &str, model: &str, effort: Option<Effort>, cx: &mut TestAppContext) -> Asking {
    let chat = chat(name, cx);
    set_to(
        &chat,
        |thread| {
            thread.model = Some(model.into());
            thread.effort = effort;
        },
        11,
        cx,
    );
    type_in(&chat, "/eff", cx);
    chat.keys("enter", cx);
    chat
}

#[gpui_kit::test]
fn a_model_keeps_the_effort_it_can_run_at(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-keeps-effort", cx);
    set_to(&chat, |thread| thread.effort = Some(Effort::High), 11, cx);

    press(&chat, "ctrl-m down enter", cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            model: Some(Some("second".into())),
            ..nothing()
        }],
    );
}

#[gpui_kit::test]
fn a_model_takes_the_nearest_effort_below_the_one_it_can_not_run_at(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-nearest-effort", cx);
    set_to(&chat, |thread| thread.effort = Some(Effort::Max), 11, cx);

    press(&chat, "ctrl-m down enter", cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            model: Some(Some("second".into())),
            effort: Some(Some(Effort::High)),
            ..nothing()
        }],
    );
}

#[gpui_kit::test]
fn the_slider_opens_on_what_shows_and_each_move_goes_to_the_host(cx: &mut TestAppContext) {
    let chat = slider_on("chat-footer-slider", "first", Some(Effort::High), cx);
    assert!(sliding(&chat, cx));
    assert_eq!(chat.typed(cx), "", "the command that opened it is gone");

    press(&chat, "left left right", cx);

    let efforts: Vec<_> = updated(&chat, cx)
        .into_iter()
        .map(|asked| asked.effort)
        .collect();
    assert_eq!(
        efforts,
        [Some(Some(Effort::Low)), Some(Some(Effort::High))],
        "it stops at the end, and the second goes though the thread, not caught up, is still at it",
    );

    press(&chat, "escape", cx);
    assert!(!sliding(&chat, cx));
    chat.keys("a", cx);
    assert_eq!(chat.typed(cx), "a", "and the keys are what's typed's again");
}

#[gpui_kit::test]
fn a_model_with_no_efforts_has_no_slider(cx: &mut TestAppContext) {
    let chat = slider_on("chat-footer-no-slider", "third", None, cx);

    assert!(!sliding(&chat, cx));
}

#[gpui_kit::test]
fn a_popover_opened_closes_the_other(cx: &mut TestAppContext) {
    let chat = slider_on("chat-footer-one-popover", "first", None, cx);

    press(&chat, "ctrl-m", cx);

    assert!(!sliding(&chat, cx));
    assert!(picking(&chat, cx));
}

#[gpui_kit::test]
fn the_slider_closes_on_another_model(cx: &mut TestAppContext) {
    let chat = slider_on("chat-footer-slider-model", "first", None, cx);

    set_to(&chat, |thread| thread.model = Some("second".into()), 12, cx);

    assert!(!sliding(&chat, cx));
}

#[gpui_kit::test]
fn what_shows_is_what_runs(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-runs", cx);
    set_to(&chat, |thread| thread.model = Some("second".into()), 11, cx);

    chat.keys("a enter", cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            effort: Some(Some(Effort::High)),
            ..nothing()
        }],
        "not set to an effort, it takes Extra high as near as its model gets, ahead of the message",
    );
}

#[gpui_kit::test]
fn a_chat_set_to_an_effort_keeps_it_when_it_sends(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-keeps-set", cx);
    set_to(&chat, |thread| thread.effort = Some(Effort::Low), 11, cx);

    chat.keys("a enter", cx);

    assert_eq!(updated(&chat, cx), []);
}

#[gpui_kit::test]
fn the_settings_choose_the_effort_of_a_chat_not_set_to_one(cx: &mut TestAppContext) {
    let chat = chat("chat-footer-setting", cx);
    cx.update(|cx| crate::settings::update(cx, |settings| settings.chat_effort = Effort::Low));
    draw(chat.window, cx);

    chat.keys("a enter", cx);

    assert_eq!(
        updated(&chat, cx),
        [UpdateParams {
            effort: Some(Some(Effort::Low)),
            ..nothing()
        }]
    );
}

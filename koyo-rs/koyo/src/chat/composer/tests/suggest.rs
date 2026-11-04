//! What `@` and `/` complete to, and the keys while they show.

use std::time::{Duration, Instant};

use gpui_kit::TestAppContext;
use koyo_agent::protocol::methods::{Catalog, Command, UpdateParams};
use koyo_agent::protocol::requests::{Questions, Request, RequestPayload, RequestStatus};
use koyo_project::Feed;

use super::{catalog, catalog_is, chat, nothing, picking, press, type_in, updated};
use crate::chat::tests::asks::{Asking, request};
use crate::chat::tests::question;
use crate::checkouts;
use crate::test_support::draw;

fn suggesting(chat: &Asking, cx: &TestAppContext) -> bool {
    chat.composer.read_with(cx, |composer, _| composer.suggesting())
}

fn with_commands(name: &str, cx: &mut TestAppContext) -> Asking {
    let chat = chat(name, cx);
    let listed = Catalog {
        commands: vec![Command {
            name: "review".into(),
            description: "Reviews what's changed".into(),
            argument_hint: Some("[what]".into()),
        }],
        ..catalog()
    };
    catalog_is(&chat, listed, cx);
    chat
}

#[gpui_kit::test]
fn a_slash_lists_the_commands_and_one_of_the_agents_goes_in_as_typed(cx: &mut TestAppContext) {
    let chat = with_commands("chat-suggest-theirs", cx);

    type_in(&chat, "/rev", cx);
    assert!(suggesting(&chat, cx));

    press(&chat, "enter", cx);

    assert_eq!(chat.typed(cx), "/review ");
    assert!(!suggesting(&chat, cx), "a caret past the word isn't in it");
}

#[gpui_kit::test]
fn tab_picks_what_is_highlighted_as_enter_does(cx: &mut TestAppContext) {
    let chat = with_commands("chat-suggest-tab", cx);

    type_in(&chat, "/", cx);
    press(&chat, "pagedown tab", cx);

    assert_eq!(chat.typed(cx), "/review ");
}

#[gpui_kit::test]
fn the_rows_have_the_arrows_while_they_show(cx: &mut TestAppContext) {
    let chat = with_commands("chat-suggest-arrows", cx);

    type_in(&chat, "/", cx);
    press(&chat, "down up down down enter", cx);

    assert_eq!(chat.typed(cx), "", "/compact is done, not said");
    let asked = chat.composer.read_with(cx, |composer, _| composer.error.clone());
    assert!(asked.is_some(), "and no host hears it here, which it says");
}

#[gpui_kit::test]
fn koyos_own_command_is_done_at_once(cx: &mut TestAppContext) {
    let chat = with_commands("chat-suggest-own", cx);

    type_in(&chat, "/mod", cx);
    press(&chat, "enter", cx);

    assert_eq!(chat.typed(cx), "");
    assert!(picking(&chat, cx), "/model opens the picker");
    assert!(!suggesting(&chat, cx));
}

#[gpui_kit::test]
fn plan_is_asked_for_by_its_command(cx: &mut TestAppContext) {
    let chat = with_commands("chat-suggest-plan", cx);

    type_in(&chat, "/plan", cx);
    press(&chat, "enter", cx);

    assert_eq!(updated(&chat, cx), [UpdateParams { plan: Some(true), ..nothing() }]);
}

#[gpui_kit::test]
fn escape_leaves_the_word_alone_until_the_caret_leaves_it(cx: &mut TestAppContext) {
    let chat = with_commands("chat-suggest-escape", cx);

    type_in(&chat, "/re", cx);
    press(&chat, "escape", cx);
    assert!(!suggesting(&chat, cx));
    assert_eq!(chat.typed(cx), "/re");

    type_in(&chat, "v", cx);
    assert!(!suggesting(&chat, cx), "typing more of it doesn't bring them back");

    type_in(&chat, " @", cx);
    assert!(chat.composer.read_with(cx, |composer, _| composer.dismissed.is_none()));
}

#[gpui_kit::test]
fn what_matches_nothing_takes_no_keys(cx: &mut TestAppContext) {
    let chat = with_commands("chat-suggest-none", cx);

    type_in(&chat, "/nothing-is-called-this", cx);

    assert!(!suggesting(&chat, cx));
    press(&chat, "enter", cx);
    let tried = chat.composer.read_with(cx, |composer, _| composer.error.clone());
    assert!(tried.is_some(), "so enter sends, which no host hears here");
}

#[gpui_kit::test]
fn the_rows_have_the_keys_ahead_of_what_is_asked_and_give_them_back(cx: &mut TestAppContext) {
    let asked = RequestPayload::Question(Questions {
        questions: vec![question("Tabs?", false, &["Tabs", "Spaces"])],
        live: true,
    });
    let pending = Request {
        payload: asked,
        ..request("q1", RequestStatus::Pending, false, 10)
    };
    let chat = Asking::new("chat-suggest-asked", vec![pending], cx);
    catalog_is(&chat, catalog(), cx);

    type_in(&chat, "/mod", cx);
    press(&chat, "enter", cx);

    assert_eq!(chat.said(cx), [], "it's what's asked that waits");
    assert!(picking(&chat, cx));
}

#[gpui_kit::test]
fn an_at_finds_the_files_of_the_checkout_and_names_the_one_picked(cx: &mut TestAppContext) {
    // The project lists the checkout on a thread of its own, from outside the test's scheduler.
    cx.executor().allow_parking();
    let chat = chat("chat-suggest-files", cx);
    let cwd = chat.composer.read_with(cx, |composer, _| composer.cwd.clone());
    let (git, dirs) = checkouts::open_git(&cwd).unwrap();
    let project = cx.update(|cx| checkouts::project(git.root(), dirs, cx));
    let updates = project.subscribe(Feed::Everything);

    type_in(&chat, "see @READ", cx);

    let deadline = Instant::now() + Duration::from_secs(30);
    while !suggesting(&chat, cx) {
        assert!(Instant::now() < deadline, "README.md is in it");
        updates.next_within(Duration::from_millis(50));
        // What follows it looks again on a timer while it's still listing.
        cx.executor().advance_clock(Duration::from_secs(1));
        draw(chat.window, cx);
    }

    press(&chat, "enter", cx);

    assert_eq!(chat.typed(cx), "see @README.md ");
}

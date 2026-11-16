//! The keyboard with a fake `AppKit` installed, told what the window tells it: presses, a page
//! asking for the keyboard, keys, menus, Koyo in front or not, and windows closing. The first
//! responder follows the holder, and goes back where it belongs on the next turn when
//! anything else moved it.

use super::fake::{Fake, Turns, installed};
use crate::keyboard::reconcile::{AppKit as _, Responder};
use crate::keyboard::state::{Cause, Holder, Part, Surface};
use crate::keyboard::{held, hooks, set_in};

const WINDOW: u64 = 1;
const PAGE: u64 = 12;
const AGENTS_PAGE: u64 = 13;
const COMPOSER: Surface = Surface::Gpui(Part::Pane(7));

/// A window with a page and an agent's page in it, the person's keyboard in the composer.
fn in_composer() -> (std::rc::Rc<Fake>, Turns) {
    let (fake, turns) = installed();
    fake.page_in(PAGE, Some(WINDOW));
    fake.page_in(AGENTS_PAGE, Some(WINDOW));
    set_in(WINDOW, COMPOSER, Cause::Focused);
    (fake, turns)
}

fn holder() -> Holder {
    held(WINDOW).unwrap().0
}

fn first(fake: &Fake) -> Responder {
    fake.first_responder(WINDOW)
}

#[test]
fn the_person_clicking_a_page_then_the_composer_has_the_keyboard_follow() {
    let (fake, turns) = in_composer();
    // Heard before the press is handed on; `AppKit` makes the page's view the first
    // responder as it hands it on, which is what's wanted by then.
    hooks::pressed(WINDOW, Responder::Page(PAGE), 0);
    assert_eq!(holder(), Holder::Page(PAGE));
    assert_eq!(first(&fake), Responder::Page(PAGE), "the reconciler moved it");
    fake.moved(WINDOW, Responder::Page(PAGE));
    // GPUI's focus moves to the composer as the press there is handled.
    hooks::pressed(WINDOW, Responder::Gpui, 0);
    assert_eq!(first(&fake), Responder::Page(PAGE), "a press alone leaves a page holding");
    set_in(WINDOW, COMPOSER, Cause::Focused);
    assert_eq!(first(&fake), Responder::Gpui);
    assert_eq!(turns.next(), 0, "nothing to put back");
}

#[test]
fn a_page_asking_for_the_keyboard_while_the_composer_has_it_is_refused() {
    let (fake, turns) = in_composer();
    fake.moved(WINDOW, Responder::Page(AGENTS_PAGE));
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)), "the holder stays");
    assert_eq!(turns.next(), 1);
    assert_eq!(first(&fake), Responder::Gpui, "taken back on the next turn");
}

#[test]
fn the_page_the_person_is_in_asking_for_the_keyboard_has_it() {
    let (fake, turns) = in_composer();
    set_in(WINDOW, Surface::Page(PAGE), Cause::Focused);
    fake.moved(WINDOW, Responder::Gpui);
    assert_eq!(turns.next(), 1, "moved off the page: put back");
    assert_eq!(first(&fake), Responder::Page(PAGE));
    fake.moved(WINDOW, Responder::Page(PAGE));
    assert_eq!(turns.next(), 0);
}

#[test]
fn the_reconcilers_own_moves_are_not_heard_as_requests() {
    let (fake, turns) = in_composer();
    set_in(WINDOW, Surface::Page(PAGE), Cause::Focused);
    set_in(WINDOW, COMPOSER, Cause::Focused);
    assert_eq!(fake.asked.borrow().len(), 2);
    assert_eq!(turns.next(), 0);
}

#[test]
fn a_page_asking_again_and_again_is_taken_back_twenty_times_a_second_at_most() {
    let (fake, turns) = in_composer();
    for _ in 0..30 {
        fake.moved(WINDOW, Responder::Page(AGENTS_PAGE));
        fake.moved(WINDOW, Responder::Gpui);
    }
    // Moved off a page back to GPUI's view by itself is no request: only the asks count.
    assert_eq!(turns.next(), 20);
}

#[test]
fn a_key_finding_a_page_with_the_composer_holding_goes_to_the_composer() {
    // An agent typing into its page out of the person's way: whatever Chromium does with the
    // first responder meanwhile, the person's keys stay in the composer, and the agent's
    // input goes to its page's renderer, past `AppKit`.
    let (fake, turns) = in_composer();
    fake.tells.set(false);
    fake.moved(WINDOW, Responder::Page(AGENTS_PAGE));
    hooks::at_key(WINDOW);
    assert_eq!(first(&fake), Responder::Gpui, "healed before the key is delivered");
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)));
    assert_eq!(turns.next(), 0);
}

#[test]
fn a_key_while_text_is_composed_in_a_page_stays_there() {
    let (fake, _) = in_composer();
    fake.tells.set(false);
    fake.moved(WINDOW, Responder::Page(PAGE));
    fake.marked.set(true);
    hooks::at_key(WINDOW);
    assert_eq!(first(&fake), Responder::Page(PAGE));
}

#[test]
fn the_side_buttons_give_no_page_the_keyboard() {
    let (_, _) = in_composer();
    hooks::pressed(WINDOW, Responder::Page(PAGE), 3);
    hooks::pressed(WINDOW, Responder::Page(PAGE), 4);
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)));
    hooks::pressed(WINDOW, Responder::Page(PAGE), 1);
    assert_eq!(holder(), Holder::Page(PAGE), "a right press does, as it opens the page's menu");
}

#[test]
fn a_menu_has_the_keys_while_it_tracks_them_and_gives_them_back() {
    let (fake, turns) = in_composer();
    hooks::menu(true);
    assert_eq!(holder(), Holder::Menu);
    // Left alone while it tracks, but for a page asking.
    fake.moved(WINDOW, Responder::Window);
    assert_eq!(turns.next(), 0);
    hooks::menu(false);
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)));
    assert_eq!(first(&fake), Responder::Gpui, "put back as the menu closed");
}

#[test]
fn another_app_in_front_and_back_gives_the_person_what_they_had() {
    let (fake, _) = in_composer();
    set_in(WINDOW, Surface::Page(PAGE), Cause::Click);
    hooks::active(false);
    assert_eq!(holder(), Holder::Away);
    fake.tells.set(false);
    fake.moved(WINDOW, Responder::Gpui);
    hooks::active(true);
    assert_eq!(holder(), Holder::Page(PAGE));
    assert_eq!(first(&fake), Responder::Page(PAGE));
}

#[test]
fn a_dialog_closing_with_the_keys_hands_them_back_to_the_window_behind() {
    let (fake, turns) = in_composer();
    // A window of Koyo's the keyboard isn't kept for, a dialog's or the tab switcher's,
    // takes the keys and closes with them.
    crate::keyboard::KEYBOARDS.with(|keyboards| keyboards.borrow_mut().key(WINDOW, false));
    assert_eq!(holder(), Holder::Away);
    hooks::closed(None, true);
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)));
    assert_eq!(turns.next(), 1);
    assert_eq!(*fake.keyed.borrow(), vec![WINDOW]);
    // One that closes without the keys changes nothing.
    hooks::closed(None, false);
    assert_eq!(turns.next(), 0);
}

#[test]
fn a_window_kept_for_closing_is_forgotten() {
    let (_, turns) = in_composer();
    hooks::closed(Some(WINDOW), true);
    assert_eq!(held(WINDOW), None);
    assert_eq!(turns.next(), 0);
}

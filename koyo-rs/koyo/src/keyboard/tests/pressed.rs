//! A press that goes on to a page gives the page the keyboard at once (`hooks::delivered`),
//! against a fake `AppKit`: the page's keys' view is the first responder before the next key,
//! whichever of the page's views took the press and whether or not it asks for the keyboard.

use super::fake::{Fake, Turns, installed};
use crate::keyboard::reconcile::{AppKit as _, Responder};
use crate::keyboard::state::{Cause, Holder, Part, Surface};
use crate::keyboard::{held, hooks, set_in};

const WINDOW: u64 = 1;
const PAGE: u64 = 12;
const COMPOSER: Surface = Surface::Gpui(Part::Pane(7));

/// A window with a page in it, the person's keyboard in the composer, a press giving a page
/// the keyboard by itself or, with `by_the_press` false, as before.
fn in_composer(by_the_press: bool) -> (std::rc::Rc<Fake>, Turns) {
    let (fake, turns) = installed();
    hooks::click_by_the_press(by_the_press);
    fake.page_in(PAGE, Some(WINDOW));
    set_in(WINDOW, COMPOSER, Cause::Focused);
    (fake, turns)
}

fn holder() -> Holder {
    held(WINDOW).unwrap().0
}

/// The press as the window tells it: heard, then, GPUI not keeping it, going on to the page.
fn press_reaches_the_page(button: isize) {
    hooks::pressed(WINDOW, Responder::Page(PAGE), button);
    hooks::delivered(WINDOW, Responder::Page(PAGE), button);
}

#[test]
fn a_press_that_reaches_a_page_gives_it_the_keyboard_though_its_view_never_asks() {
    let (fake, turns) = in_composer(true);
    // The page's outer view took the press: nothing asks for the first responder.
    press_reaches_the_page(0);
    assert_eq!(holder(), Holder::Page(PAGE));
    assert_eq!(fake.first_responder(WINDOW), Responder::Page(PAGE), "before the next key");
    // The key right after goes to the page, with nothing to heal.
    hooks::at_key(WINDOW);
    assert_eq!(fake.first_responder(WINDOW), Responder::Page(PAGE));
    assert_eq!(turns.next(), 0, "nothing to put back");
}

#[test]
fn before_the_page_had_it_only_once_its_view_asked_and_a_key_meanwhile_was_koyos() {
    let (fake, _) = in_composer(false);
    press_reaches_the_page(0);
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)), "still the composer's");
    hooks::at_key(WINDOW);
    assert_eq!(fake.first_responder(WINDOW), Responder::Gpui, "the key lost to the page");
}

#[test]
fn a_press_gpui_keeps_gives_the_page_nothing() {
    let (fake, turns) = in_composer(true);
    // A divider beside the page, a button drawn over it: heard, and never going on.
    hooks::pressed(WINDOW, Responder::Page(PAGE), 0);
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)));
    hooks::at_key(WINDOW);
    assert_eq!(fake.first_responder(WINDOW), Responder::Gpui);
    // The page asking after it is refused, as any page that wasn't pressed.
    fake.moved(WINDOW, Responder::Page(PAGE));
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)));
    assert_eq!(turns.next(), 1);
    assert_eq!(fake.first_responder(WINDOW), Responder::Gpui);
}

#[test]
fn the_right_button_gives_a_page_the_keyboard_as_the_left_does_and_a_side_button_doesnt() {
    let (_, _) = in_composer(true);
    press_reaches_the_page(3);
    assert_eq!(holder(), Holder::Gpui(Part::Pane(7)), "a side button steps through history");
    press_reaches_the_page(1);
    assert_eq!(holder(), Holder::Page(PAGE));
}

#[test]
fn a_press_on_a_page_that_holds_already_puts_a_strayed_first_responder_back_on_it() {
    let (fake, _) = in_composer(true);
    press_reaches_the_page(0);
    fake.tells.set(false);
    fake.moved(WINDOW, Responder::Gpui);
    fake.tells.set(true);
    press_reaches_the_page(0);
    assert_eq!(fake.first_responder(WINDOW), Responder::Page(PAGE));
}

#[test]
fn a_press_that_reaches_a_page_whose_views_are_in_no_window_leaves_the_keys_with_gpui() {
    let (fake, _) = in_composer(true);
    fake.page_in(PAGE, None);
    press_reaches_the_page(0);
    assert_eq!(holder(), Holder::Page(PAGE), "the person put the keyboard there");
    assert_eq!(fake.first_responder(WINDOW), Responder::Gpui, "until its views are in");
}

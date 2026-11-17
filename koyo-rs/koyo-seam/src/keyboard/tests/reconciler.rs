//! The reconciler, against a fake `AppKit`: the first responder each holder wants, put there
//! when it isn't, at a key unless text is being composed, and at a press on GPUI's content.

use super::fake::Fake;
use crate::keyboard::reconcile::{self, AppKit as _, Heard, Moved, Responder};
use crate::keyboard::state::{Holder, Part, Surface};

const WINDOW: u64 = 1;
const OTHER_WINDOW: u64 = 2;
const PAGE: u64 = 12;
const OTHER_PAGE: u64 = 13;
const COMPOSER: Holder = Holder::Gpui(Part::Pane(7));

fn fake() -> Fake {
    let fake = Fake::default();
    fake.page_in(PAGE, Some(WINDOW));
    fake.page_in(OTHER_PAGE, Some(WINDOW));
    fake
}

#[test]
fn each_holder_wants_its_own_view_or_none() {
    let fake = fake();
    assert_eq!(reconcile::wanted(COMPOSER, WINDOW, &fake), Some(Responder::Gpui));
    assert_eq!(reconcile::wanted(Holder::Page(PAGE), WINDOW, &fake), Some(Responder::Page(PAGE)));
    assert_eq!(reconcile::wanted(Holder::Menu, WINDOW, &fake), None);
    assert_eq!(reconcile::wanted(Holder::Away, WINDOW, &fake), None);
}

#[test]
fn a_page_whose_views_left_the_window_has_gpuis_view_in_its_place_until_they_are_back() {
    let fake = fake();
    fake.page_in(PAGE, Some(OTHER_WINDOW));
    assert_eq!(reconcile::wanted(Holder::Page(PAGE), WINDOW, &fake), Some(Responder::Gpui));
    fake.moved(WINDOW, Responder::Page(PAGE));
    let left = reconcile::reconcile(Holder::Page(PAGE), WINDOW, &fake);
    let gpui = Moved { before: Responder::Page(PAGE), after: Responder::Gpui };
    assert_eq!(left, Some(gpui));
    fake.page_in(PAGE, Some(WINDOW));
    let back = reconcile::reconcile(Holder::Page(PAGE), WINDOW, &fake);
    assert_eq!(back, Some(Moved { before: Responder::Gpui, after: Responder::Page(PAGE) }));
}

#[test]
fn reconciling_what_is_already_there_asks_nothing() {
    let fake = fake();
    assert_eq!(reconcile::reconcile(COMPOSER, WINDOW, &fake), None);
    assert_eq!(reconcile::reconcile(Holder::Menu, WINDOW, &fake), None);
    assert_eq!(reconcile::reconcile(Holder::Away, WINDOW, &fake), None);
    assert!(fake.asked.borrow().is_empty());
}

#[test]
fn a_key_finding_the_first_responder_elsewhere_hands_it_over_first() {
    let fake = fake();
    fake.moved(WINDOW, Responder::Page(PAGE));
    let healed = reconcile::at_key(COMPOSER, WINDOW, &fake);
    assert_eq!(healed, Some(Moved { before: Responder::Page(PAGE), after: Responder::Gpui }));
    assert_eq!(fake.first_responder(WINDOW), Responder::Gpui);
}

#[test]
fn a_key_while_text_is_being_composed_goes_where_the_composition_is() {
    let fake = fake();
    fake.moved(WINDOW, Responder::Page(PAGE));
    fake.marked.set(true);
    assert_eq!(reconcile::at_key(COMPOSER, WINDOW, &fake), None);
    assert_eq!(fake.first_responder(WINDOW), Responder::Page(PAGE));
    assert!(fake.asked.borrow().is_empty());
}

#[test]
fn a_key_going_where_it_should_is_left_alone() {
    let fake = fake();
    assert_eq!(reconcile::at_key(COMPOSER, WINDOW, &fake), None);
    fake.moved(WINDOW, Responder::Page(PAGE));
    assert_eq!(reconcile::at_key(Holder::Page(PAGE), WINDOW, &fake), None);
    assert_eq!(reconcile::at_key(Holder::Menu, WINDOW, &fake), None);
    assert_eq!(reconcile::at_key(Holder::Away, WINDOW, &fake), None);
    assert!(fake.asked.borrow().is_empty());
}

#[test]
fn a_press_on_gpuis_content_gives_it_the_keyboard_back_if_it_should_have_it() {
    let fake = fake();
    fake.moved(WINDOW, Responder::Page(PAGE));
    let healed = reconcile::at_press(COMPOSER, WINDOW, &fake);
    assert_eq!(healed, Some(Moved { before: Responder::Page(PAGE), after: Responder::Gpui }));
    fake.moved(WINDOW, Responder::Page(PAGE));
    assert_eq!(reconcile::at_press(Holder::Page(PAGE), WINDOW, &fake), None);
}

#[test]
fn a_page_asking_for_the_keyboard_has_it_only_if_the_person_put_it_there() {
    let fake = fake();
    let composer = Surface::Gpui(Part::Pane(7));
    let asked = |holder, surface, pane| {
        reconcile::heard(holder, surface, Responder::Page(pane), WINDOW, &fake)
    };
    assert_eq!(asked(COMPOSER, composer, PAGE), Heard::Refused(PAGE));
    assert_eq!(asked(Holder::Page(PAGE), Surface::Page(PAGE), PAGE), Heard::Wanted);
    assert_eq!(asked(Holder::Page(PAGE), Surface::Page(PAGE), OTHER_PAGE), Heard::Refused(OTHER_PAGE));
    assert_eq!(asked(Holder::Away, Surface::Page(PAGE), PAGE), Heard::Wanted);
    assert_eq!(asked(Holder::Away, composer, PAGE), Heard::Refused(PAGE));
    assert_eq!(asked(Holder::Menu, composer, PAGE), Heard::Refused(PAGE));
}

#[test]
fn anything_else_moving_it_off_what_is_wanted_is_astray_unless_the_keys_are_elsewhere() {
    let fake = fake();
    let heard = |holder, after| {
        reconcile::heard(holder, Surface::Page(PAGE), after, WINDOW, &fake)
    };
    assert_eq!(heard(Holder::Page(PAGE), Responder::Gpui), Heard::Astray);
    assert_eq!(heard(Holder::Page(PAGE), Responder::Window), Heard::Astray);
    assert_eq!(heard(COMPOSER, Responder::Gpui), Heard::Wanted);
    assert_eq!(heard(Holder::Menu, Responder::Window), Heard::Wanted);
    assert_eq!(heard(Holder::Away, Responder::Other), Heard::Wanted);
}

#[test]
fn put_back_while_away_it_is_what_the_person_had() {
    let fake = fake();
    fake.moved(WINDOW, Responder::Page(OTHER_PAGE));
    let back = reconcile::restore(Holder::Away, Surface::Page(PAGE), WINDOW, &fake);
    let page = Moved { before: Responder::Page(OTHER_PAGE), after: Responder::Page(PAGE) };
    assert_eq!(back, Some(page));
}

//! The window's native views taken out and put back (`hooks::remounted`): a page that had the
//! first responder gets it back on the next turn, before a key has to find it astray.

use super::fake::installed;
use crate::keyboard::reconcile::{AppKit as _, Responder};
use crate::keyboard::state::{Cause, Holder, Part, Surface};
use crate::keyboard::{held, hooks, set_in};

const WINDOW: u64 = 1;
const PAGE: u64 = 12;

fn written(with: &str) -> usize {
    crate::keyboard::lines::WRITTEN.with(|lines| {
        lines.borrow().iter().filter(|line| line.contains(with)).count()
    })
}

#[test]
fn a_page_that_lost_the_first_responder_unheard_has_it_back_on_the_next_turn() {
    let (fake, turns) = installed();
    fake.page_in(PAGE, Some(WINDOW));
    set_in(WINDOW, Surface::Page(PAGE), Cause::Click);
    assert_eq!(fake.first_responder(WINDOW), Responder::Page(PAGE));
    fake.tells.set(false);
    fake.moved(WINDOW, Responder::Window);
    fake.tells.set(true);
    hooks::remounted(WINDOW);
    assert_eq!(fake.first_responder(WINDOW), Responder::Window, "not inside the frame");
    assert_eq!(turns.next(), 1);
    assert_eq!(fake.first_responder(WINDOW), Responder::Page(PAGE));
    assert_eq!(held(WINDOW).unwrap().0, Holder::Page(PAGE));
    hooks::at_key(WINDOW);
    assert_eq!(written("healed"), 0);
    assert_eq!(written("views were put back"), 1);
}

#[test]
fn views_put_back_while_gpui_has_the_keyboard_move_nothing() {
    let (fake, turns) = installed();
    fake.page_in(PAGE, Some(WINDOW));
    set_in(WINDOW, Surface::Gpui(Part::Pane(7)), Cause::Focused);
    hooks::remounted(WINDOW);
    assert_eq!(turns.next(), 1);
    assert_eq!(fake.first_responder(WINDOW), Responder::Gpui);
    assert_eq!(written("views were put back"), 0);
}

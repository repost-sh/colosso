//! The holders, told what happens as the app tells them, and the log's line for each move.

use super::lines::{self, Followed};
use super::state::{Cause, Change, Holder, Keyboards, Part, Surface};

const WINDOW: u64 = 1;
const PAGE: u64 = 12;
const OTHER_PAGE: u64 = 13;
const COMPOSER: Part = Part::Pane(7);

/// A window whose keyboard the person put in `surface`.
fn holding(surface: Surface) -> Keyboards {
    let mut keyboards = Keyboards::default();
    keyboards.set(WINDOW, surface, Cause::Focused);
    keyboards
}

fn holder(keyboards: &Keyboards) -> Holder {
    keyboards.holder(WINDOW).unwrap()
}

#[test]
fn a_window_first_heard_of_has_its_own_keys() {
    let mut keyboards = Keyboards::default();
    assert_eq!(keyboards.key(WINDOW, true), None);
    assert_eq!(holder(&keyboards), Holder::Gpui(Part::Window));
}

#[test]
fn the_person_moving_from_a_page_to_the_composer_moves_the_holder_each_time() {
    let mut keyboards = holding(Surface::Gpui(COMPOSER));
    let into = keyboards.set(WINDOW, Surface::Page(PAGE), Cause::Focused);
    assert_eq!(
        into,
        Some(Change {
            window: WINDOW,
            from: Holder::Gpui(COMPOSER),
            to: Holder::Page(PAGE),
            cause: Cause::Focused,
        })
    );
    let out = keyboards.left_page(WINDOW, PAGE, COMPOSER);
    assert_eq!(out.map(|change| change.to), Some(Holder::Gpui(COMPOSER)));
    assert_eq!(holder(&keyboards), Holder::Gpui(COMPOSER));
}

#[test]
fn the_same_holder_again_is_no_move() {
    let mut keyboards = holding(Surface::Page(PAGE));
    assert_eq!(keyboards.set(WINDOW, Surface::Page(PAGE), Cause::TabShown), None);
}

#[test]
fn a_page_left_after_another_took_the_keyboard_changes_nothing() {
    // Two pages' focus moves are heard in no set order: the one taking it first, here.
    let mut keyboards = holding(Surface::Page(PAGE));
    keyboards.set(WINDOW, Surface::Page(OTHER_PAGE), Cause::Focused);
    assert_eq!(keyboards.left_page(WINDOW, PAGE, Part::Other), None);
    assert_eq!(holder(&keyboards), Holder::Page(OTHER_PAGE));
}

#[test]
fn a_window_not_key_holds_nothing_and_gets_back_what_it_had() {
    let mut keyboards = holding(Surface::Page(PAGE));
    let away = keyboards.key(WINDOW, false).unwrap();
    assert_eq!((away.from, away.to, away.cause), (Holder::Page(PAGE), Holder::Away, Cause::WindowResigned));
    let back = keyboards.key(WINDOW, true).unwrap();
    assert_eq!((back.from, back.to, back.cause), (Holder::Away, Holder::Page(PAGE), Cause::WindowKey));
}

#[test]
fn what_the_person_does_while_away_is_what_comes_back() {
    let mut keyboards = holding(Surface::Page(PAGE));
    keyboards.key(WINDOW, false);
    // Heard while away: no move, the window still has nothing.
    assert_eq!(keyboards.set(WINDOW, Surface::Gpui(COMPOSER), Cause::TabShown), None);
    assert_eq!(holder(&keyboards), Holder::Away);
    assert_eq!(keyboards.key(WINDOW, true).map(|change| change.to), Some(Holder::Gpui(COMPOSER)));
}

#[test]
fn a_dialog_has_the_keys_until_it_closes_and_the_window_is_key_again() {
    let mut keyboards = holding(Surface::Gpui(COMPOSER));
    let opened = keyboards.dialog(WINDOW, true).unwrap();
    assert_eq!((opened.to, opened.cause), (Holder::Away, Cause::DialogOpened));
    // The dialog's window takes the key window's place.
    assert_eq!(keyboards.key(WINDOW, false), None);
    // Closed, it leaves the window away until the window is key again.
    assert_eq!(keyboards.dialog(WINDOW, false), None);
    let back = keyboards.key(WINDOW, true).unwrap();
    assert_eq!((back.to, back.cause), (Holder::Gpui(COMPOSER), Cause::WindowKey));
}

#[test]
fn a_dialog_closed_with_its_window_still_key_gives_the_keys_back_at_once() {
    let mut keyboards = holding(Surface::Page(PAGE));
    keyboards.dialog(WINDOW, true);
    let back = keyboards.dialog(WINDOW, false).unwrap();
    assert_eq!((back.to, back.cause), (Holder::Page(PAGE), Cause::DialogClosed));
}

#[test]
fn a_page_closed_with_the_keyboard_in_it_leaves_the_window_its_own_keys() {
    let mut keyboards = holding(Surface::Page(PAGE));
    keyboards.set(2, Surface::Page(OTHER_PAGE), Cause::Focused);
    let changes = keyboards.page_closed(PAGE);
    assert_eq!(changes.len(), 1, "only the window it was in");
    assert_eq!((changes[0].to, changes[0].cause), (Holder::Gpui(Part::Window), Cause::PageClosed));
    assert_eq!(keyboards.holder(2), Some(Holder::Page(OTHER_PAGE)));
    assert!(keyboards.page_closed(PAGE).is_empty(), "once");
}

#[test]
fn a_line_names_what_had_it_what_has_it_and_why() {
    let change = Change {
        window: WINDOW,
        from: Holder::Gpui(COMPOSER),
        to: Holder::Page(PAGE),
        cause: Cause::Focused,
    };
    assert_eq!(lines::line(&change, Followed::Untold), "keyboard: gpui (pane 7) → page 12 (focus moved)");
    assert_eq!(lines::level(&change), log::Level::Info);
    let bar = Change {
        from: Holder::Page(PAGE),
        to: Holder::Gpui(Part::Bar(PAGE)),
        ..change
    };
    assert_eq!(lines::line(&bar, Followed::Untold), "keyboard: page 12 → gpui (a bar of page 12) (focus moved)");
}

#[test]
fn going_away_and_dialogs_are_warnings() {
    let away = Change {
        window: WINDOW,
        from: Holder::Page(PAGE),
        to: Holder::Away,
        cause: Cause::WindowResigned,
    };
    assert_eq!(lines::line(&away, Followed::Untold), "keyboard: page 12 → away (window resigned)");
    assert_eq!(lines::level(&away), log::Level::Warn);
    let back = Change {
        from: Holder::Away,
        to: Holder::Gpui(Part::Window),
        cause: Cause::DialogClosed,
        ..away
    };
    assert_eq!(lines::level(&back), log::Level::Warn);
}

#[test]
fn a_line_says_what_the_first_responder_did_and_never_more() {
    use super::reconcile::{Moved, Responder};
    let change = Change {
        window: WINDOW,
        from: Holder::Gpui(COMPOSER),
        to: Holder::Page(PAGE),
        cause: Cause::Click,
    };
    let moved = Moved { before: Responder::Gpui, after: Responder::Page(PAGE) };
    assert_eq!(
        lines::line(&change, Followed::Moved(moved)),
        "keyboard: gpui (pane 7) → page 12 (click); first responder gpui → page 12"
    );
    assert_eq!(
        lines::line(&change, Followed::Stayed),
        "keyboard: gpui (pane 7) → page 12 (click); first responder as it was"
    );
    let asked = Moved { before: Responder::Gpui, after: Responder::Page(OTHER_PAGE) };
    assert_eq!(
        lines::refused(OTHER_PAGE, Holder::Gpui(COMPOSER), asked),
        "keyboard: page 13 asked for the keyboard (gpui → page 13) while gpui (pane 7) has it: \
         refused"
    );
    let healed = Moved { before: Responder::Window, after: Responder::Gpui };
    assert_eq!(
        lines::healed(Holder::Gpui(COMPOSER), "a key found the first responder elsewhere", healed),
        "keyboard: gpui (pane 7) has it, a key found the first responder elsewhere; first \
         responder the window → gpui: healed"
    );
}

#[test]
fn a_tab_shown_again_has_what_it_had_as_it_was_left() {
    let mut keyboards = holding(Surface::Gpui(COMPOSER));
    let (pages, chat) = (100, 200);
    assert_eq!(keyboards.tab_shown(WINDOW, pages), None, "nothing kept yet");
    keyboards.set(WINDOW, Surface::Page(PAGE), Cause::Click);
    assert_eq!(keyboards.tab_shown(WINDOW, chat), None);
    keyboards.set(WINDOW, Surface::Gpui(COMPOSER), Cause::Click);
    assert_eq!(keyboards.tab_shown(WINDOW, pages), Some(Surface::Page(PAGE)));
    keyboards.set(WINDOW, Surface::Page(PAGE), Cause::TabShown);
    assert_eq!(keyboards.tab_shown(WINDOW, chat), Some(Surface::Gpui(COMPOSER)));
    keyboards.set(WINDOW, Surface::Gpui(COMPOSER), Cause::TabShown);
    // The tab shown, asked for again, has what it has now.
    keyboards.set(WINDOW, Surface::Gpui(Part::Window), Cause::Focused);
    assert_eq!(keyboards.tab_shown(WINDOW, chat), Some(Surface::Gpui(Part::Window)));
    // A page that closed is kept for no tab.
    keyboards.page_closed(PAGE);
    assert_eq!(keyboards.tab_shown(WINDOW, pages), None);
}

#[test]
fn what_the_person_asked_for_is_the_lines_cause() {
    use super::state::Intent;
    let change = Change {
        window: WINDOW,
        from: Holder::Page(PAGE),
        to: Holder::Gpui(Part::Bar(PAGE)),
        cause: Cause::Intent(Intent::Address),
    };
    assert_eq!(
        lines::line(&change, Followed::Untold),
        "keyboard: page 12 → gpui (a bar of page 12) (⌘L)"
    );
    let switched = Change { cause: Cause::Intent(Intent::Switcher), ..change };
    assert!(lines::line(&switched, Followed::Untold).ends_with("(⌃⇥)"));
}

mod fake;
mod inactive;
mod reconciler;
mod told;

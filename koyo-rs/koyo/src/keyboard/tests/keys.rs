//! One key, one owner: with a page holding the keyboard, Koyo's tab and window keys are Koyo's
//! and never the page's; every other key Koyo binds is the page's first, and Koyo's once the
//! page leaves it, once. The offers are the vendored window's (`gpui_macos::responders`), the
//! say over who's first the keyboard's, against a fake `AppKit`.

use gpui_kit::{Keystroke, TestAppContext};
use gpui_macos::responders::{Answer, Offerer, Offers};

use super::fake::installed;
use crate::keyboard::reconcile::{AppKit as _, Responder};
use crate::keyboard::reserved::{RESERVED, reserved};
use crate::keyboard::state::{Cause, Part, Surface};
use crate::keyboard::{hooks, set_in};

const WINDOW: u64 = 1;
const PAGE: u64 = 12;

fn key(source: &'static str) -> Keystroke {
    Keystroke::parse(source).unwrap()
}

/// A window whose page has the keyboard.
fn page_holding() -> std::rc::Rc<super::fake::Fake> {
    let (fake, _) = installed();
    fake.page_in(PAGE, Some(WINDOW));
    set_in(WINDOW, Surface::Page(PAGE), Cause::Click);
    assert_eq!(fake.first_responder(WINDOW), Responder::Page(PAGE));
    fake
}

/// What GPUI's view does with the event of time `at` for `keystroke`, offered by `from`.
fn offered(offers: &mut Offers, at: f64, from: Offerer, keystroke: &Keystroke) -> Answer {
    offers.again(at, from).unwrap_or_else(|| offers.first(at, hooks::gpui_first(WINDOW, keystroke)))
}

#[test]
fn command_l_with_a_page_holding_is_koyos_and_never_the_pages() {
    let _fake = page_holding();
    let mut offers = Offers::default();
    // With Koyo in front, `AppKit`'s own pass offers it first: GPUI takes it, `AppKit` stops,
    // and the page never sees it.
    assert_eq!(offered(&mut offers, 1., Offerer::AppKit, &key("cmd-l")), Answer::Dispatch);
    assert_eq!(offered(&mut offers, 1., Offerer::Router, &key("cmd-l")), Answer::Decline, "once");
    // With Koyo behind (the agent desktop), the window's routing offers it before the page.
    assert_eq!(offered(&mut offers, 2., Offerer::Router, &key("cmd-l")), Answer::Dispatch);
    assert_eq!(offered(&mut offers, 2., Offerer::AppKit, &key("cmd-l")), Answer::Decline, "handed back");
}

#[test]
fn a_key_koyo_binds_outside_its_own_is_the_pages_first_and_koyos_once_the_page_leaves_it() {
    let _fake = page_holding();
    let mut offers = Offers::default();
    let bold = key("cmd-b");
    // The page handles it: Koyo's never offered it again, and never has it.
    assert_eq!(offered(&mut offers, 1., Offerer::AppKit, &bold), Answer::Decline);
    assert_eq!(offered(&mut offers, 1., Offerer::Router, &bold), Answer::Decline);
    // The page leaves it: handed back, it's Koyo's, once.
    assert_eq!(offered(&mut offers, 2., Offerer::AppKit, &bold), Answer::Decline);
    assert_eq!(offered(&mut offers, 2., Offerer::Router, &bold), Answer::Decline);
    assert_eq!(offered(&mut offers, 2., Offerer::AppKit, &bold), Answer::Dispatch, "left");
    assert_eq!(offered(&mut offers, 2., Offerer::AppKit, &bold), Answer::Decline, "once");
}

#[test]
fn with_gpui_holding_every_key_koyo_binds_is_koyos_first() {
    let (fake, _) = installed();
    fake.page_in(PAGE, Some(WINDOW));
    set_in(WINDOW, Surface::Gpui(Part::Pane(7)), Cause::Click);
    assert!(hooks::gpui_first(WINDOW, &key("cmd-b")));
    assert!(hooks::gpui_first(WINDOW, &key("cmd-l")));
}

#[test]
fn the_keys_koyo_keeps_are_its_tab_and_window_keys_and_no_others() {
    for kept in ["cmd-t", "cmd-w", "cmd-n", "cmd-l", "cmd-shift-t", "cmd-q", "ctrl-tab"] {
        assert!(reserved(&key(kept)), "{kept}");
    }
    assert!(reserved(&key("ctrl-shift-tab")));
    for left in ["cmd-b", "cmd-k", "cmd-enter", "cmd-f", "cmd-c", "cmd-1", "alt-up", "l"] {
        assert!(!reserved(&key(left)), "{left}");
    }
}

/// Each key Koyo keeps is one it binds: the list is checked against the keymap.
#[gpui_kit::test]
fn each_key_koyo_keeps_is_bound_in_its_keymap(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        for kept in RESERVED {
            let wanted = key(kept);
            let bound = keymap.bindings().any(|binding| {
                let [stroke] = binding.keystrokes() else {
                    return false;
                };
                let stroke = stroke.inner();
                stroke.modifiers == wanted.modifiers && stroke.key == wanted.key
            });
            assert!(bound, "{kept} is bound");
        }
    });
}

//! A page's views moving, as the app's keyboard hears of it, and what its browser tells
//! Chromium of it.

use std::cell::Cell;

use objc2_foundation::NSSize;

use super::parts::mark;
use super::settle::{Spot, spot};
use super::{Entry, PAGES, PaneId, Views, closed, page};

thread_local! {
    /// What the keyboard last heard: the page, how its views move, and whether the page was
    /// still known then.
    static HEARD: Cell<Option<(PaneId, Views, bool)>> = const { Cell::new(None) };
}

/// What the host tells the layer's keyboard, noted as it's told.
pub(super) fn heard(pane: PaneId, views: Views) {
    HEARD.set(Some((pane, views, page(pane, |_| ()).is_some())));
}

/// A page with none of its views made, as it is before CEF makes them.
pub(crate) fn opened(pane: PaneId) {
    let entry = Entry {
        holder: None,
        views: None,
        shown_in: None,
        home: None,
        kept: true,
        seen: true,
        on_screen: true,
        size: NSSize::new(800., 600.),
    };
    PAGES.with(|pages| pages.borrow_mut().insert(pane, entry));
}

#[test]
fn a_page_closing_tells_the_keyboard_while_the_page_is_still_there() {
    opened(7);
    closed(7);
    assert_eq!(HEARD.get(), Some((7, Views::Leaving, true)));
    assert!(page(7, |_| ()).is_none(), "and then it's gone");
    HEARD.set(None);
    closed(7);
    assert_eq!(HEARD.get(), None, "a page closed already tells nothing");
}

/// A page a pane stops drawing that no agent keeps leaves every window at once: it isn't
/// parked in its holder first, which woke it and hid it again as a tab was left.
#[test]
fn a_page_no_pane_draws_and_no_agent_keeps_is_in_no_window() {
    assert_eq!(spot(false, false, true), Spot::Nowhere, "a tab the person left");
    assert_eq!(spot(false, true, true), Spot::Holder, "a page an agent uses out of sight");
    assert_eq!(spot(true, false, true), Spot::Pane, "a tab the person sees");
    assert_eq!(spot(true, false, false), Spot::Nowhere, "its window covered or minimized");
    assert_eq!(spot(true, true, false), Spot::Pane, "an agent uses it, its window unseen");
}

/// Chromium keeps the extensions' actions, popups and shortcuts of a browser marked for them
/// only: a page's, while the host talks to the engine about extensions. An extension's
/// shortcut in a `DevTools` pane would run its action with the frontend for its tab, and one
/// with no pipe would run where the app shows nothing of it.
#[test]
fn only_a_page_is_marked_for_its_extensions_and_only_with_the_pipe_open() {
    assert_eq!(mark(false, true), Some("koyo_extensions"), "a page, the pipe open");
    assert_eq!(mark(true, true), None, "a DevTools pane or window, the pipe open");
    assert_eq!(mark(false, false), None, "a page, no pipe to the engine");
    assert_eq!(mark(true, false), None, "a DevTools pane, no pipe to the engine");
}

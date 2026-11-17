//! A page's views moving, as the app's keyboard hears of it.

use std::cell::Cell;

use objc2_foundation::NSSize;

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
fn opened(pane: PaneId) {
    let entry = Entry {
        holder: None,
        views: None,
        shown_in: None,
        home: None,
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

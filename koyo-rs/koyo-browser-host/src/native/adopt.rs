//! Tabs extensions open (`chrome.tabs.create`), taken in as panes (`docs/extensions.md` 3).
//! Chrome offers such a tab as it offers a page's `window.open`; the host gives it a pane's
//! number and a client, tells the app to give it a tab, and puts the browser view Chrome then
//! makes in a holder of its own, as a page the app opened has.

use std::cell::RefCell;
use std::collections::VecDeque;

use cef::{BrowserView, Client};
use koyo_browser_ipc::{HostEvent, PaneId};
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;

use super::{Entry, HolderDelegate, PAGES, page};

thread_local! {
    /// The pages extensions are opening, in the order Chrome asked for them: each has its
    /// pane and its client, and gets its browser view next.
    static ADOPTING: RefCell<VecDeque<PaneId>> = const { RefCell::new(VecDeque::new()) };
}

/// Forgets them, as CEF is about to stop.
pub(super) fn clear() {
    ADOPTING.with(|adopting| adopting.borrow_mut().clear());
}

/// An extension opens a tab from `opener`'s page (`chrome.tabs.create`), which Chrome offers
/// as it offers a page's `window.open` (`docs/extensions.md` 3). The tab becomes a pane: the
/// host gives it a number, the app is told to give it a tab, and Chrome makes its browser
/// with the client answered here, in a holder of its own once its view is made
/// ([`adopted`]). None when the page can't take one, and Chrome opens the tab its own way.
pub(crate) fn adopt(opener: PaneId, url: &str, active: bool) -> Option<Client> {
    // Its opener's size until a pane places it.
    let size = page(opener, |entry| entry.size)?;
    let pane = crate::host::adopt(opener, url)?;
    let entry = Entry {
        holder: None,
        views: None,
        shown_in: None,
        home: None,
        on_screen: true,
        size,
    };
    PAGES.with(|pages| pages.borrow_mut().insert(pane, entry));
    ADOPTING.with(|adopting| adopting.borrow_mut().push_back(pane));
    crate::host::send(&HostEvent::TabOpened { pane, opener, url: url.to_owned(), active });
    Some(crate::pane::client(pane, false))
}

/// The holder of `pane`'s page: the window Chrome's own bubbles for the page are children of.
pub(crate) fn holder_of(pane: PaneId) -> Option<Retained<NSWindow>> {
    page(pane, |entry| entry.holder.as_ref().map(|(_, holder)| holder.clone()))?
}

/// The pane of the page an extension is opening, whose browser view Chrome makes next.
pub(super) fn adopting() -> Option<PaneId> {
    ADOPTING.with(|adopting| adopting.borrow().front().copied())
}

/// Chrome made the browser view of the page an extension is opening: it goes in a holder of
/// its own, as a page the app opened does. Whether there was such a page to take it.
pub(super) fn adopted(view: &BrowserView) -> bool {
    let Some(pane) = ADOPTING.with(|adopting| adopting.borrow_mut().pop_front()) else {
        return false;
    };
    let mut holder = HolderDelegate::new(pane, view.clone());
    let made = cef::window_create_top_level(Some(&mut holder)).is_some();
    if !made {
        PAGES.with(|pages| pages.borrow_mut().remove(&pane));
        crate::host::failed_to_open(pane, false);
    }
    made
}

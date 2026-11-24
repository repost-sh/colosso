//! Pages a page opens (`pane/popups.rs`) and tabs extensions open (`chrome.tabs.create`,
//! `docs/extensions.md` 3), taken in as panes. Chrome offers both as it offers a page's
//! `window.open`; the host gives the page a pane's number and a client, tells the app to give it
//! a tab, and puts the browser view Chrome then makes in a holder of its own, as a page the app
//! opened has.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::os::raw::c_int;

use cef::{BrowserView, Client, ImplBrowser as _, ImplBrowserHost as _, ImplBrowserView as _, ImplClient as _};
use koyo_browser_ipc::{HostEvent, PaneId};

use super::{Entry, HolderDelegate, PAGES, page};

/// A page Chrome is making for a page that opened it: whose popup it is, its pane, and the
/// client it was given, which Chrome makes its browser with.
struct Adopting {
    opener: PaneId,
    /// Chrome's number for the popup, among the opener's.
    popup: c_int,
    pane: PaneId,
    /// The client's address ([`key`]).
    client: usize,
}

thread_local! {
    /// The pages being opened, in the order Chrome asked for them, each known by its client:
    /// Chrome may make their browser views in another order, and gives each the client it was
    /// given.
    static ADOPTING: RefCell<VecDeque<Adopting>> = const { RefCell::new(VecDeque::new()) };
}

/// How a client is known: the address of CEF's object for it, which CEF hands back as it is.
fn key(client: &Client) -> usize {
    client.get_raw() as usize
}

/// Forgets them, as CEF is about to stop.
pub(super) fn clear() {
    ADOPTING.with(|adopting| adopting.borrow_mut().clear());
}

/// `opener`'s page opens a page at `url`, Chrome's popup `popup`: a link or a script's
/// (`pane/popups.rs`), or an extension's tab (`chrome.tabs.create`, `docs/extensions.md` 3).
/// It becomes a pane: the host gives it a number, the app is told to give it a tab, shown when
/// `active`, and Chrome makes its browser with the client answered here, in a holder of its own
/// once its view is made ([`adopted`]). None when the page can't take one.
pub(crate) fn adopt(opener: PaneId, popup: c_int, url: &str, active: bool) -> Option<Client> {
    let size = page(opener, |entry| entry.size)?;
    let pane = crate::host::adopt(opener, url)?;
    let entry = Entry {
        holder: None,
        views: None,
        shown_in: None,
        home: None,
        kept: true,
        seen: true,
        on_screen: true,
        size,
    };
    PAGES.with(|pages| pages.borrow_mut().insert(pane, entry));
    let client = crate::pane::client(pane, false);
    let adopting = Adopting { opener, popup, pane, client: key(&client) };
    ADOPTING.with(|pages| pages.borrow_mut().push_back(adopting));
    crate::host::send(&HostEvent::TabOpened { pane, opener, url: url.to_owned(), active });
    Some(client)
}

/// Chrome gave up on making `opener`'s popup `popup` (`OnBeforePopupAborted`): its pane goes,
/// and the tab the app gave it.
pub(crate) fn abandoned(opener: PaneId, popup: c_int) {
    let gone = ADOPTING.with(|adopting| {
        let mut adopting = adopting.borrow_mut();
        let at = adopting.iter().position(|page| page.opener == opener && page.popup == popup)?;
        adopting.remove(at).map(|page| page.pane)
    });
    if let Some(pane) = gone {
        PAGES.with(|pages| pages.borrow_mut().remove(&pane));
        crate::host::never_made(pane);
    }
}

/// The pane of the page being opened that Chrome makes a browser view for with `client`.
pub(super) fn adopting(client: &Client) -> Option<PaneId> {
    pane_of(key(client))
}

/// The pane of the page being opened whose client is at `client`.
fn pane_of(client: usize) -> Option<PaneId> {
    ADOPTING.with(|adopting| {
        let adopting = adopting.borrow();
        adopting.iter().find(|page| page.client == client).map(|page| page.pane)
    })
}

/// The page being opened whose client is at `client`, no longer being opened.
fn taken(client: usize) -> Option<PaneId> {
    ADOPTING.with(|adopting| {
        let mut adopting = adopting.borrow_mut();
        let at = adopting.iter().position(|page| page.client == client)?;
        adopting.remove(at).map(|page| page.pane)
    })
}

/// Chrome made the browser view of a page being opened: it goes in a holder of its own, as a
/// page the app opened does. Whether there was such a page to take it, the one whose client
/// the view's browser has.
pub(super) fn adopted(view: &BrowserView) -> bool {
    let client = (view.browser()).and_then(|browser| browser.host()?.client());
    let Some(pane) = client.and_then(|client| taken(key(&client))) else {
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

#[cfg(test)]
mod tests {
    use super::{ADOPTING, Adopting, pane_of, taken};

    fn opening(pane: u64, client: usize) {
        let made = Adopting { opener: 1, popup: 0, pane, client };
        ADOPTING.with(|adopting| adopting.borrow_mut().push_back(made));
    }

    #[test]
    fn a_popups_view_goes_to_the_pane_whose_client_its_browser_has_whatever_the_order() {
        opening(10, 0x1000);
        opening(20, 0x2000);
        assert_eq!(pane_of(0x2000), Some(20), "the second made first");
        assert_eq!(taken(0x2000), Some(20));
        assert_eq!(pane_of(0x2000), None, "taken once");
        assert_eq!(taken(0x3000), None, "a browser of no page being opened");
        assert_eq!(taken(0x1000), Some(10));
    }
}

//! Pages a page opens: a link to a new tab, a ⌘-, ⇧- or middle-click, `window.open`, a
//! sign-in's popup. Chrome says where the person meant the page to go
//! (`WindowOpenDisposition`, from the link's target, the window's features and the keys held),
//! and Koyo, which has tabs and no windows for pages, gives each new tab or window a tab beside
//! the opener's.
//!
//! A page a script or a link's target opens is Chrome's to make (`OnBeforePopup` allowing it),
//! so it keeps `window.opener`: a sign-in's popup posts its answer back and closes itself. The
//! host takes it in as a pane of its own (`native::adopt`), as it takes an extension's tab. A
//! plain link the person opens a way of their own, ⌘- or middle-clicked, comes as a navigation
//! instead (`OnOpenURLFromTab`), which Chrome opens with no opener: a web page's opens beside
//! as a link the app opens does (`HostEvent::OpenUrl`), and any other kind is left to Chrome,
//! which refuses what a page may not open (a `data:` document). A popup Chrome made may move and resize its
//! window (`window.moveTo`, `resizeTo`), which here is the page's holder, which only the app
//! places: the pane's display handler refuses it (`OnContentsBoundsChange`).
//!
//! Chromium's popup blocker sees a `window.open` before the host does
//! (`ChromeContentBrowserClient::CanCreateWindow` runs it ahead of CEF's `OnBeforePopup`), and
//! a navigation after (`Browser::OpenURLFromTab`, behind CEF's `OnOpenURLFromTab`): so a
//! navigation with no gesture is left to Chrome, and a popup with no gesture that reaches the
//! host all the same is refused, as Chrome's default refuses it.

use cef::{Client, DictionaryValue, WindowOpenDisposition};
use koyo_browser_ipc::{HostEvent, PaneId};
use std::os::raw::c_int;

use crate::host;

/// Where a page a page opens goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Opening {
    /// In the opener's own tab.
    Here,
    /// In a tab of its own beside the opener's: shown, or left behind it.
    Tab { front: bool },
    /// Nowhere: a popup no gesture asked for, or what Koyo has no place for (a document's
    /// picture in picture, a download, a private window).
    Nowhere,
}

/// Where a page Chrome opens as `disposition` goes, the person having asked for it with a
/// gesture or not: a tab or a window of its own is a tab beside, in front unless Chrome would
/// leave it behind (⌘-click, a middle click).
pub(super) fn opening(disposition: WindowOpenDisposition, gesture: bool) -> Opening {
    match disposition {
        WindowOpenDisposition::CURRENT_TAB => Opening::Here,
        WindowOpenDisposition::NEW_BACKGROUND_TAB if gesture => Opening::Tab { front: false },
        WindowOpenDisposition::NEW_FOREGROUND_TAB
        | WindowOpenDisposition::NEW_POPUP
        | WindowOpenDisposition::NEW_WINDOW
        | WindowOpenDisposition::NEW_SPLIT_VIEW
        | WindowOpenDisposition::SINGLETON_TAB
        | WindowOpenDisposition::SWITCH_TO_TAB
            if gesture =>
        {
            Opening::Tab { front: true }
        }
        _ => Opening::Nowhere,
    }
}

/// Takes the page `opener`'s page opens as Chrome's popup `popup` in as a pane of its own, with
/// a tab beside the opener's, shown when `front`: Chrome makes its browser with the client and
/// the `extra_info` answered here, a page's (`native::extra_info`). Whether it was taken; with
/// no page to take it, it opens nowhere.
pub(super) fn take(
    opener: PaneId,
    popup: c_int,
    url: &str,
    front: bool,
    client: Option<&mut Option<Client>>,
    extra_info: Option<&mut Option<DictionaryValue>>,
) -> bool {
    let url = if url.is_empty() { "about:blank" } else { url };
    let (Some(client), Some(extra_info)) = (client, extra_info) else {
        return false;
    };
    let Some(tab) = crate::native::adopt(opener, popup, url, front) else {
        return false;
    };
    *client = Some(tab);
    *extra_info = crate::native::extra_info(false);
    true
}

/// `opener`'s page opens `url` as Chrome's popup `popup`, which goes where `opening` says:
/// whether Chrome makes it.
pub(super) fn before_popup(
    opener: PaneId,
    popup: c_int,
    url: String,
    opening: Opening,
    client: Option<&mut Option<Client>>,
    extra_info: Option<&mut Option<DictionaryValue>>,
) -> bool {
    match opening {
        Opening::Tab { front } => take(opener, popup, &url, front, client, extra_info),
        Opening::Here => {
            if web(&url) {
                host::navigate_later(opener, url);
            }
            false
        }
        Opening::Nowhere => false,
    }
}

/// A link `pane`'s page opens as `disposition`: one for a tab or a window of its own, asked for
/// with a gesture, opens in a tab beside the page's, and the rest as Chrome opens them. Whether
/// it was taken from Chrome.
pub(super) fn from_tab(pane: PaneId, url: &str, disposition: WindowOpenDisposition, gesture: bool) -> bool {
    let Some(front) = beside(url, disposition, gesture) else {
        return false;
    };
    host::send(&HostEvent::OpenUrl {
        pane,
        url: url.to_owned(),
        front,
    });
    true
}

/// Whether the host opens `url` for a page, or for its `DevTools`, as the browser's own
/// navigation, which Chrome checks less than a page's: a web page only, in a tab beside
/// (`from_tab`), in the page's own tab (`Opening::Here`) or from `DevTools`. Any other kind is
/// Chrome's to open or refuse as the page's own navigation (a page may open no `data:`
/// document, say), and `DevTools` open none.
pub(super) fn web(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Whether a link to `url` a page opens as `disposition` opens in a tab the app opens beside
/// it, and if so whether in front: a web page's only ([`web`]).
fn beside(url: &str, disposition: WindowOpenDisposition, gesture: bool) -> Option<bool> {
    match opening(disposition, gesture) {
        Opening::Tab { front } if web(url) => Some(front),
        Opening::Tab { .. } | Opening::Here | Opening::Nowhere => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW: [WindowOpenDisposition; 6] = [
        WindowOpenDisposition::NEW_FOREGROUND_TAB,
        WindowOpenDisposition::NEW_POPUP,
        WindowOpenDisposition::NEW_WINDOW,
        WindowOpenDisposition::NEW_SPLIT_VIEW,
        WindowOpenDisposition::SINGLETON_TAB,
        WindowOpenDisposition::SWITCH_TO_TAB,
    ];

    #[test]
    fn a_tab_or_a_window_the_person_asks_for_is_a_tab_in_front() {
        for disposition in NEW {
            assert_eq!(opening(disposition, true), Opening::Tab { front: true }, "{disposition:?}");
        }
    }

    #[test]
    fn a_background_tab_is_a_tab_left_behind() {
        let behind = opening(WindowOpenDisposition::NEW_BACKGROUND_TAB, true);
        assert_eq!(behind, Opening::Tab { front: false });
    }

    #[test]
    fn the_current_tab_is_the_opener_itself() {
        assert_eq!(opening(WindowOpenDisposition::CURRENT_TAB, true), Opening::Here);
        assert_eq!(opening(WindowOpenDisposition::CURRENT_TAB, false), Opening::Here);
    }

    #[test]
    fn a_tab_or_a_window_no_gesture_asked_for_opens_nowhere() {
        for disposition in NEW.into_iter().chain([WindowOpenDisposition::NEW_BACKGROUND_TAB]) {
            assert_eq!(opening(disposition, false), Opening::Nowhere, "{disposition:?}");
        }
    }

    #[test]
    fn what_koyo_has_no_place_for_opens_nowhere() {
        for disposition in [
            WindowOpenDisposition::NEW_PICTURE_IN_PICTURE,
            WindowOpenDisposition::SAVE_TO_DISK,
            WindowOpenDisposition::OFF_THE_RECORD,
            WindowOpenDisposition::IGNORE_ACTION,
            WindowOpenDisposition::UNKNOWN,
        ] {
            assert_eq!(opening(disposition, true), Opening::Nowhere, "{disposition:?}");
        }
    }

    #[test]
    fn the_host_opens_only_web_pages_for_a_page_or_its_devtools() {
        assert!(web("https://example.com/") && web("http://localhost:3000/"));
        for url in [
            "",
            "data:text/html,<h1>Sign in</h1>",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "chrome://settings/",
            "devtools://devtools/bundled/inspector.html",
            "blob:https://example.com/0f6c",
            "view-source:https://example.com/",
        ] {
            assert!(!web(url), "{url}");
        }
    }

    #[test]
    fn only_a_web_page_a_page_links_to_opens_in_a_tab_the_app_opens() {
        let opened = |url| beside(url, WindowOpenDisposition::NEW_BACKGROUND_TAB, true);
        assert_eq!(opened("https://example.com/"), Some(false));
        assert_eq!(opened("http://example.com/"), Some(false));
        for url in [
            "data:text/html,<h1>Sign in</h1>",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "chrome://settings/",
            "blob:https://example.com/0f6c",
        ] {
            assert_eq!(opened(url), None, "{url} is left to Chrome");
        }
    }
}

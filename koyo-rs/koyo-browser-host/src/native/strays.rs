//! Browsers Chrome makes on its own, which only a Chrome-style engine does: none is a pane's,
//! so each gets the client made here (`docs/extensions.md` 3).
//!
//! - A **window an extension asked for** (`chrome.windows.create` with type `popup` or
//!   `panel`: a wallet's confirmation, a sign-in, a popup popped out) stays a window of
//!   Chrome's, as Chrome draws it. It's kept count of, and closed as the engine stops.
//! - A **tabbed window** has no place in Koyo: Chrome makes one for a tab it has nowhere to
//!   put (an extension's options, a page it opens after it's removed). The address it goes
//!   to opens as a tab of the app's, and the window is closed before it shows.
//!
//! Chrome tells a client nothing of a browser's kind, and a browser just made has no window
//! yet. By its first page it has, and the window says: a tabbed one draws its tabs in its
//! title bar, so its content takes the whole window (`FullSizeContentView`); a popup's has a
//! title bar of its own over its page. Seen on Chromium 156, 2026-11-16.
//!
//! On an engine without Koyo's patches, where extensions have no controls of the app's
//! (`extensions::on`), every such browser is closed as soon as it's made, as before.

use std::cell::RefCell;

use cef::{
    Browser, Client, Frame, ImplBrowser, ImplBrowserHost, ImplClient, ImplLifeSpanHandler,
    ImplRequest, ImplRequestHandler, LifeSpanHandler, Request, RequestHandler, WrapClient,
    WrapLifeSpanHandler, WrapRequestHandler, rc::Rc, wrap_client, wrap_life_span_handler,
    wrap_request_handler,
};
use koyo_browser_ipc::HostEvent;
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow, NSWindowStyleMask};
use std::os::raw::c_int;

thread_local! {
    /// The browsers Chrome made on its own that are still open: CEF's identifier of each,
    /// the browser, and what it is, once its window has said.
    static STRAYS: RefCell<Vec<(c_int, Browser, Option<Kind>)>> = const { RefCell::new(Vec::new()) };
}

/// The client of a browser Chrome makes on its own.
pub(crate) fn client() -> Client {
    StrayClient::new()
}

/// How many of them are open.
pub(crate) fn count() -> usize {
    STRAYS.with(|strays| strays.borrow().len())
}

/// Closes them all, as the engine stops.
pub(crate) fn close_all() {
    let open: Vec<Browser> =
        STRAYS.with(|strays| strays.borrow().iter().map(|(_, browser, _)| browser.clone()).collect());
    for browser in open {
        if let Some(host) = browser.host() {
            host.close_browser(1);
        }
    }
}

/// Lets go of them, as CEF is about to stop.
pub(super) fn clear() {
    STRAYS.with(|strays| strays.borrow_mut().clear());
}

/// How opaque `window` is, from 0, clear, to 1.
pub(crate) fn alpha(window: &NSWindow) -> f64 {
    // SAFETY: `NSWindow`'s own getter, a `CGFloat`.
    unsafe { objc2::msg_send![window, alphaValue] }
}

/// Makes `window` that opaque: clear at 0, where it's still there and takes no part. So a
/// window of Chrome's own stays unseen until it's known what it is and where it goes.
pub(crate) fn set_alpha(window: &NSWindow, alpha: f64) {
    // SAFETY: `NSWindow`'s own setter, a `CGFloat`.
    let _: () = unsafe { objc2::msg_send![window, setAlphaValue: alpha] };
}

/// The window `browser` is in.
fn window_of(browser: &Browser) -> Option<Retained<NSWindow>> {
    let host = browser.host()?;
    // SAFETY: on macOS a browser's window handle is an `NSView`, which lives with it.
    let view: Option<&NSView> = unsafe { host.window_handle().cast::<NSView>().as_ref() };
    view.and_then(NSView::window)
}

/// What a browser Chrome made on its own is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A tabbed window, whose page becomes a tab of the app's.
    Tabbed,
    /// A window an extension asked for, which stays Chrome's.
    Window,
}

/// What a window of Chrome's with this style is: a tabbed one's content takes all of it.
fn kind(style: NSWindowStyleMask) -> Kind {
    if style.contains(NSWindowStyleMask::FullSizeContentView) { Kind::Tabbed } else { Kind::Window }
}

/// What `browser` is, read from its window once and kept; none while it has no window.
fn kind_of(browser: &Browser) -> Option<Kind> {
    let id = browser.identifier();
    let known = STRAYS
        .with(|strays| strays.borrow().iter().find(|(stray, ..)| *stray == id).map(|(.., kind)| *kind));
    // None: not one of them (an engine without the patches closes each as it's made).
    let known = known?;
    if known.is_some() {
        return known;
    }
    let window = window_of(browser)?;
    let found = kind(window.styleMask());
    log::info!(
        "the browser Chrome made is {}",
        match found {
            Kind::Tabbed => "a tabbed window, whose page becomes a tab",
            Kind::Window => "a window of its own, which stays",
        }
    );
    if found == Kind::Tabbed {
        // Never seen: it closes as it says where it was going.
        set_alpha(&window, 0.);
        window.setIgnoresMouseEvents(true);
        window.orderOut(None);
    }
    STRAYS.with(|strays| {
        if let Some(entry) = strays.borrow_mut().iter_mut().find(|(stray, ..)| *stray == id) {
            entry.2 = Some(found);
        }
    });
    Some(found)
}

wrap_client! {
    struct StrayClient;

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(StrayLife::new())
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(StrayRequests::new())
        }
    }
}

wrap_life_span_handler! {
    struct StrayLife;

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser else {
                return;
            };
            if !crate::extensions::on() {
                log::info!("Chrome made a browser on its own: closed");
                if let Some(host) = browser.host() {
                    host.close_browser(1);
                }
                return;
            }
            log::info!("Chrome made a browser on its own");
            STRAYS.with(|strays| {
                strays.borrow_mut().push((browser.identifier(), browser.clone(), None));
            });
            // It has no window yet, as a rule: what it is is read at its first page.
            kind_of(browser);
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                let id = browser.identifier();
                STRAYS.with(|strays| strays.borrow_mut().retain(|(stray, ..)| *stray != id));
                // The last of them to close may be all a quitting host waited for.
                crate::host::quit_when_closed();
            }
        }
    }
}

wrap_request_handler! {
    struct StrayRequests;

    impl RequestHandler {
        // A tabbed window's first page: it opens as a tab of the app's instead, of the
        // window's own profile, beside the page the person is on when that's of it, else
        // beside another of that profile's; and the window goes.
        fn on_before_browse(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: c_int,
            _is_redirect: c_int,
        ) -> c_int {
            let (Some(browser), Some(frame), Some(request)) = (browser, frame, request) else {
                return 0;
            };
            if cef::ImplFrame::is_main(frame) == 0 || kind_of(browser) != Some(Kind::Tabbed) {
                return 0;
            }
            let url = cef::CefString::from(&request.url()).to_string();
            if !url.is_empty() && url != "about:blank" {
                let beside = (browser.host())
                    .and_then(|host| host.request_context())
                    .and_then(|mut profile| {
                        crate::host::pane_of_profile(&mut profile, crate::extensions::active_pane())
                    });
                match beside {
                    // Chrome would have shown its window: the tab is shown.
                    Some(pane) => crate::host::send(&HostEvent::OpenUrl { pane, url, front: true }),
                    None => log::warn!("a page Chrome opened has no pane of its profile to open beside: {url}"),
                }
            }
            if let Some(host) = browser.host() {
                host.close_browser(1);
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_whose_content_takes_all_of_it_is_tabbed() {
        // As Chromium 156 makes them: a popup's window, and a tabbed one.
        let popup = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        assert_eq!(kind(popup), Kind::Window);
        assert_eq!(kind(popup | NSWindowStyleMask::FullSizeContentView), Kind::Tabbed);
    }
}

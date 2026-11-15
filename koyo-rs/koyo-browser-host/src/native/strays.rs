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
//! Chrome tells a client nothing of a browser's kind. A popup's window shows its title in a
//! title bar of its own; a tabbed window's title bar holds its tabs, and shows no title.
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
use objc2_app_kit::{NSView, NSWindow, NSWindowTitleVisibility};
use std::os::raw::c_int;

thread_local! {
    /// The browsers Chrome made on its own that are still open: CEF's identifier of each,
    /// the browser, and whether it's a tabbed window, to be closed once it says where it goes.
    static STRAYS: RefCell<Vec<(c_int, Browser, bool)>> = const { RefCell::new(Vec::new()) };
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

/// Whether `window` is a tabbed window of Chrome's: its title bar holds tabs, and no title.
fn is_tabbed(window: &NSWindow) -> bool {
    window.titleVisibility() != NSWindowTitleVisibility::Visible
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
            let window = window_of(browser);
            let kept = crate::extensions::on();
            let tabbed = !kept || window.as_deref().is_none_or(is_tabbed);
            log::info!(
                "Chrome made a browser on its own: {}",
                if !kept {
                    "closed"
                } else if tabbed {
                    "a tabbed window, whose page becomes a tab"
                } else {
                    "a window, which stays"
                }
            );
            if tabbed && let Some(window) = &window {
                // Never seen: it closes once it says where it was going.
                set_alpha(window, 0.);
                window.setIgnoresMouseEvents(true);
                window.orderOut(None);
            }
            if !kept {
                if let Some(host) = browser.host() {
                    host.close_browser(1);
                }
                return;
            }
            STRAYS.with(|strays| {
                strays.borrow_mut().push((browser.identifier(), browser.clone(), tabbed));
            });
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
            let id = browser.identifier();
            let tabbed = STRAYS.with(|strays| {
                strays.borrow().iter().any(|(stray, _, tabbed)| *stray == id && *tabbed)
            });
            if !tabbed || cef::ImplFrame::is_main(frame) == 0 {
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
                    Some(pane) => crate::host::send(&HostEvent::OpenUrl { pane, url }),
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

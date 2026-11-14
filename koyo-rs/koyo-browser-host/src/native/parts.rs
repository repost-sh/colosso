//! A native page's delegates, which make it a Chrome-style browser in a frameless window, and
//! the client of browsers Chrome makes on its own.

use cef::{
    BrowserView, BrowserViewDelegate, ChromeToolbarType, Client, ImplBrowser, ImplBrowserHost,
    ImplBrowserView,
    ImplBrowserViewDelegate, ImplClient, ImplLifeSpanHandler, ImplPanelDelegate, ImplViewDelegate,
    ImplWindowDelegate, LifeSpanHandler, Rect, RuntimeStyle, Window, WindowDelegate,
    WrapBrowserViewDelegate, WrapClient, WrapLifeSpanHandler, WrapWindowDelegate, rc::Rc,
    wrap_browser_view_delegate, wrap_client, wrap_life_span_handler, wrap_window_delegate,
};
use koyo_browser_ipc::PaneId;
use objc2_app_kit::NSView;
use std::os::raw::c_int;

use super::{window_created, window_destroyed};

wrap_browser_view_delegate! {
    pub(super) struct PageViewDelegate {
        pane: PaneId,
    }

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn chrome_toolbar_type(&self, _browser_view: Option<&mut BrowserView>) -> ChromeToolbarType {
            ChromeToolbarType::NONE
        }

        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }
    }
}

wrap_window_delegate! {
    pub(super) struct HolderDelegate {
        pane: PaneId,
        view: BrowserView,
    }

    impl ViewDelegate {}

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            if let Some(window) = window {
                window_created(self.pane, window, &self.view);
            }
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            window_destroyed();
        }

        // The window may close once its browser says so, which a browser asked to close
        // does. (The wrapper's default is no, where CEF's own is yes.)
        fn can_close(&self, _window: Option<&mut Window>) -> c_int {
            (self.view.browser())
                .and_then(|browser| browser.host())
                .map_or(1, |host| host.try_close_browser())
        }

        fn initial_bounds(&self, _window: Option<&mut Window>) -> Rect {
            Rect { x: 0, y: 0, width: 800, height: 600 }
        }

        fn is_frameless(&self, _window: Option<&mut Window>) -> c_int {
            1
        }

        fn with_standard_window_buttons(&self, _window: Option<&mut Window>) -> c_int {
            0
        }

        fn can_resize(&self, _window: Option<&mut Window>) -> c_int {
            0
        }

        fn can_maximize(&self, _window: Option<&mut Window>) -> c_int {
            0
        }

        fn can_minimize(&self, _window: Option<&mut Window>) -> c_int {
            0
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }
    }
}

/// The client of a browser Chrome makes on its own (a window it brings back, one an extension
/// opens): for now such a browser is closed as soon as it's made, and its window never shown.
pub(crate) fn stray_client() -> Client {
    StrayClient::new()
}

wrap_client! {
    struct StrayClient;

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(StrayLife::new())
        }
    }
}

wrap_life_span_handler! {
    struct StrayLife;

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut cef::Browser>) {
            let Some(host) = browser.and_then(|browser| browser.host()) else {
                return;
            };
            log::warn!("closing a browser Chrome made on its own");
            // SAFETY: on macOS a browser's window handle is an `NSView`, which lives with it.
            let view: Option<&NSView> = unsafe { host.window_handle().cast::<NSView>().as_ref() };
            if let Some(window) = view.and_then(NSView::window) {
                // SAFETY: `NSWindow`'s own setter, a `CGFloat`.
                let _: () = unsafe { objc2::msg_send![&*window, setAlphaValue: 0.0f64] };
                window.setIgnoresMouseEvents(true);
                window.orderOut(None);
            }
            host.close_browser(1);
        }
    }
}

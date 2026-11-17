//! A native page's delegates, which make it a Chrome-style browser in a frameless window.

use cef::{
    BrowserSettings, BrowserView, BrowserViewDelegate, ChromeToolbarType, Client, ImplBrowser,
    ImplBrowserHost, ImplBrowserView, ImplBrowserViewDelegate, ImplPanelDelegate,
    ImplView, ImplViewDelegate, ImplWindowDelegate, Rect, RuntimeStyle, Window, WindowDelegate,
    WrapBrowserViewDelegate, WrapWindowDelegate, rc::Rc, wrap_browser_view_delegate,
    wrap_window_delegate,
};
use koyo_browser_ipc::PaneId;
use std::os::raw::c_int;

use super::{adopted, adopting, window_created, window_destroyed};

/// Hides Chrome's toolbar on `view`, where the page has one: it's there for extensions, whose
/// popups anchor to it, and never seen.
pub(super) fn hide_toolbar(view: &BrowserView) {
    if let Some(toolbar) = view.chrome_toolbar() {
        toolbar.set_visible(0);
    }
}

wrap_browser_view_delegate! {
    pub(super) struct PageViewDelegate {
        pane: PaneId,
        toolbar: bool,
    }

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn chrome_toolbar_type(&self, _browser_view: Option<&mut BrowserView>) -> ChromeToolbarType {
            if self.toolbar {
                ChromeToolbarType::NORMAL
            } else {
                ChromeToolbarType::NONE
            }
        }

        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }

        fn delegate_for_popup_browser_view(
            &self,
            _browser_view: Option<&mut BrowserView>,
            _settings: Option<&BrowserSettings>,
            _client: Option<&mut Client>,
            is_devtools: c_int,
        ) -> Option<BrowserViewDelegate> {
            if is_devtools != 0 {
                return None;
            }
            adopting().map(|pane| PageViewDelegate::new(pane, self.toolbar))
        }

        fn on_popup_browser_view_created(
            &self,
            _browser_view: Option<&mut BrowserView>,
            popup_browser_view: Option<&mut BrowserView>,
            is_devtools: c_int,
        ) -> c_int {
            match popup_browser_view {
                Some(view) if is_devtools == 0 => c_int::from(adopted(view)),
                _ => 0,
            }
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
            window_destroyed(self.pane);
        }

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

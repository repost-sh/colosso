//! A native page's delegates, which make it a Chrome-style browser in a frameless window, and
//! what its browser tells Chromium of it.

use cef::{
    BrowserSettings, BrowserView, BrowserViewDelegate, ChromeToolbarType, Client, DictionaryValue,
    ImplBrowser, ImplBrowserHost, ImplBrowserView, ImplBrowserViewDelegate, ImplDictionaryValue,
    ImplPanelDelegate, ImplViewDelegate, ImplWindowDelegate, Rect, RuntimeStyle, Window,
    WindowDelegate, WrapBrowserViewDelegate, WrapWindowDelegate, rc::Rc, wrap_browser_view_delegate,
    wrap_window_delegate,
};
use koyo_browser_ipc::PaneId;
use std::os::raw::c_int;

use super::{adopted, adopting, window_created, window_destroyed};

/// The `extra_info` of a browser the host makes for a pane or lets CEF make from one, a page's
/// or a `DevTools` one's: its [`mark`], if it has one. Always a dictionary of the host's own,
/// empty with no mark: a `DevTools` window CEF makes starts with its page's, and the cef crate
/// keeps what CEF offered when the answer is none.
pub(crate) fn extra_info(devtools: bool) -> Option<DictionaryValue> {
    let info = cef::dictionary_value_create()?;
    if let Some(mark) = mark(devtools, crate::extensions::on()) {
        info.set_bool(Some(&mark.into()), 1);
    }
    Some(info)
}

/// The mark in a browser's `extra_info` that has Chromium keep its extensions' actions with no
/// toolbar of Chrome's (`extensions-without-a-toolbar.patch`), so that the app's icons press
/// them, their popups open and their shortcuts work: a page's, not a `DevTools` one's, whose
/// frontend an action would take for its tab, and only while the host talks to the engine
/// about extensions (`extensions::on`), as with no pipe the app draws no icons. No page can
/// set it: a page names its popup's frame, not its `extra_info`.
pub(super) fn mark(devtools: bool, extensions: bool) -> Option<&'static str> {
    (extensions && !devtools).then_some("koyo_extensions")
}

wrap_browser_view_delegate! {
    pub(super) struct PageViewDelegate {
        pane: PaneId,
    }

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        /// No toolbar of Chrome's: the app draws the page's controls, its extensions' icons
        /// among them, and Chrome keeps the extensions of a page marked for them without one
        /// ([`mark`]). One carried hidden was laid out again on every frame of its holder,
        /// shown or not (`docs/native-pages.md` 5).
        fn chrome_toolbar_type(&self, _browser_view: Option<&mut BrowserView>) -> ChromeToolbarType {
            ChromeToolbarType::NONE
        }

        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }

        fn delegate_for_popup_browser_view(
            &self,
            _browser_view: Option<&mut BrowserView>,
            _settings: Option<&BrowserSettings>,
            client: Option<&mut Client>,
            is_devtools: c_int,
        ) -> Option<BrowserViewDelegate> {
            if is_devtools != 0 {
                return None;
            }
            adopting(client?).map(PageViewDelegate::new)
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

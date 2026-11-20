//! One browser pane's CEF client. Every handler carries the pane id and
//! reports to [`crate::host`], which owns the pane's state on the UI thread.
use crate::host;
use cef::{
    Browser, BrowserSettings, CefString, CefStringList, Client, ContextMenuHandler,
    ContextMenuParams, CursorInfo, CursorType, DialogHandler, DictionaryValue, DisplayHandler,
    FindHandler, Frame, FrameHandler, ImplClient, ImplContextMenuHandler, ImplDictionaryValue,
    ImplDisplayHandler, ImplFindHandler, ImplFrameHandler, ImplLifeSpanHandler, ImplLoadHandler,
    ImplProcessMessage, JsdialogHandler, LifeSpanHandler, LoadHandler, MenuModel,
    PermissionHandler, PopupFeatures, ProcessId, ProcessMessage, Rect, RequestHandler,
    RunContextMenuCallback, TransitionType, WindowInfo, WindowOpenDisposition, WrapClient,
    WrapContextMenuHandler, WrapDisplayHandler, WrapFindHandler, WrapFrameHandler,
    WrapLifeSpanHandler, WrapLoadHandler, rc::Rc, wrap_client, wrap_context_menu_handler,
    wrap_display_handler, wrap_find_handler, wrap_frame_handler, wrap_life_span_handler,
    wrap_load_handler,
};
use koyo_browser_ipc::{Cursor, HostEvent, PaneId};
use std::os::raw::c_int;

mod favicon;
mod keys;
mod popups;
mod prompts;

/// The client a pane's browser is created with; `devtools` for a `DevTools`
/// pane, whose links open in new tabs of the app rather than in it.
pub(crate) fn client(pane: PaneId, devtools: bool) -> Client {
    PaneClient::new(pane, devtools)
}

wrap_client! {
    struct PaneClient {
        pane: PaneId,
        devtools: bool,
    }

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(PaneDisplay::new(self.pane))
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(PaneLoad::new(self.pane))
        }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(PaneLifeSpan::new(self.pane, self.devtools))
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(prompts::requests(self.pane))
        }
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            Some(prompts::script_dialogs(self.pane))
        }
        fn dialog_handler(&self) -> Option<DialogHandler> {
            Some(prompts::files(self.pane))
        }
        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(prompts::permissions(self.pane))
        }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(PaneMenus::new(self.pane))
        }
        fn find_handler(&self) -> Option<FindHandler> {
            Some(PaneFind::new(self.pane))
        }
        fn frame_handler(&self) -> Option<FrameHandler> {
            Some(PaneFrames::new(self.pane))
        }
        fn download_handler(&self) -> Option<cef::DownloadHandler> {
            Some(crate::downloads::handler(self.pane))
        }
        fn keyboard_handler(&self) -> Option<cef::KeyboardHandler> {
            Some(keys::keys(self.pane))
        }
        fn on_process_message_received(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> c_int {
            let (Some(browser), Some(frame), Some(message)) = (browser, frame, message) else {
                return 0;
            };
            if source_process == ProcessId::BROWSER {
                let asked = CefString::from(&message.name()).to_string() == "Koyo.activate";
                if asked {
                    host::send(&HostEvent::TabActivated { pane: self.pane });
                }
                return c_int::from(asked);
            }
            if crate::session::from_renderer(self.pane, message)
                || crate::tone::from_renderer(self.pane, message)
            {
                return 1;
            }
            c_int::from(crate::passkeys::from_renderer(self.pane, browser, frame, message))
        }
    }
}

wrap_frame_handler! {
    struct PaneFrames {
        pane: PaneId,
    }

    impl FrameHandler {
        fn on_frame_detached(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>) {
            if let Some(browser) = browser {
                crate::passkeys::frames_changed(self.pane, browser, frame.map(|f| &*f));
            }
        }

        fn on_main_frame_changed(
            &self,
            browser: Option<&mut Browser>,
            _old_frame: Option<&mut Frame>,
            _new_frame: Option<&mut Frame>,
        ) {
            if let Some(browser) = browser {
                crate::passkeys::frames_changed(self.pane, browser, None);
            }
        }
    }
}

wrap_find_handler! {
    struct PaneFind {
        pane: PaneId,
    }

    impl FindHandler {
        fn on_find_result(
            &self,
            _browser: Option<&mut Browser>,
            _identifier: c_int,
            count: c_int,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: c_int,
            _final_update: c_int,
        ) {
            host::found(self.pane, count, active_match_ordinal);
        }
    }
}

wrap_context_menu_handler! {
    struct PaneMenus {
        pane: PaneId,
    }

    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            if let Some(model) = model {
                crate::menus::prepare(model);
            }
        }

        fn run_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
            callback: Option<&mut RunContextMenuCallback>,
        ) -> c_int {
            let (Some(params), Some(model), Some(callback)) = (params, model, callback) else {
                return 0;
            };
            crate::menus::run(self.pane, params, model, callback.clone());
            1
        }
    }
}

wrap_display_handler! {
    struct PaneDisplay {
        pane: PaneId,
    }

    impl DisplayHandler {
        fn on_address_change(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            use cef::ImplFrame;
            if frame.is_some_and(|f| f.is_main() == 1) {
                let url = text(url);
                host::page_changed(self.pane, |page| page.url.clone_from(&url));
                if let Some(browser) = browser.as_deref() {
                    crate::session::history_changed(self.pane, browser);
                }
                crate::location::opened(browser, &url);
            }
        }

        fn on_loading_progress_change(&self, _browser: Option<&mut Browser>, progress: f64) {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0 through 100 first"
            )]
            let percent = (progress.clamp(0., 1.) * 100.).round() as u8;
            host::progressed(self.pane, percent);
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            host::page_changed(self.pane, |page| {
                page.title = title.map(ToString::to_string).unwrap_or_default();
            });
            if let Some(browser) = browser {
                crate::session::history_changed(self.pane, browser);
            }
        }

        fn on_favicon_urlchange(
            &self,
            browser: Option<&mut Browser>,
            icon_urls: Option<&mut CefStringList>,
        ) {
            let urls = crate::dialogs::strings(icon_urls);
            let first = urls.first().cloned().unwrap_or_default();
            let named = host::page_changed(self.pane, |page| page.favicon = first);
            if let (true, Some(browser)) = (named, browser) {
                favicon::fetch(self.pane, browser, urls);
            }
        }

        fn on_contents_bounds_change(
            &self,
            _browser: Option<&mut Browser>,
            _new_bounds: Option<&Rect>,
        ) -> c_int {
            1
        }

        fn on_fullscreen_mode_change(&self, _browser: Option<&mut Browser>, fullscreen: c_int) {
            host::fullscreen_changed(self.pane, fullscreen != 0);
        }

        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: *mut u8,
            type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> c_int {
            host::cursor_changed(self.pane, cursor(type_));
            0
        }
    }
}

wrap_load_handler! {
    struct PaneLoad {
        pane: PaneId,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            browser: Option<&mut Browser>,
            is_loading: c_int,
            can_go_back: c_int,
            can_go_forward: c_int,
        ) {
            host::page_changed(self.pane, |page| {
                page.loading = is_loading != 0;
                page.can_go_back = can_go_back != 0;
                page.can_go_forward = can_go_forward != 0;
            });
            host::loading_changed(self.pane, is_loading != 0);
            if let Some(browser) = browser {
                crate::session::history_changed(self.pane, browser);
            }
        }

        fn on_load_start(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            if let (Some(browser), Some(frame)) = (browser, frame) {
                crate::passkeys::load_started(self.pane, browser, frame);
                if cef::ImplFrame::is_main(frame) == 1 {
                    crate::session::left(self.pane);
                }
            }
        }
    }
}

wrap_life_span_handler! {
    struct PaneLifeSpan {
        pane: PaneId,
        devtools: bool,
    }

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            popup_id: c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            user_gesture: c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut c_int>,
        ) -> c_int {
            let extension_tab = (extra_info.as_deref().and_then(Option::as_ref))
                .is_some_and(|info| info.has_key(Some(&"koyo_extension_tab".into())) != 0);
            let url = text(target_url);
            if extension_tab {
                let front = target_disposition != WindowOpenDisposition::NEW_BACKGROUND_TAB;
                return c_int::from(!popups::take(self.pane, popup_id, &url, front, client));
            }
            if self.devtools {
                if popups::web(&url) {
                    host::send(&HostEvent::OpenUrl { pane: self.pane, url, front: false });
                }
                return 1;
            }
            let opening = popups::opening(target_disposition, user_gesture != 0);
            c_int::from(!popups::before_popup(self.pane, popup_id, url, opening, client))
        }

        fn on_before_popup_aborted(&self, _browser: Option<&mut Browser>, popup_id: c_int) {
            crate::native::abandoned(self.pane, popup_id);
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                host::browser_created(self.pane, browser);
            }
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            host::browser_closed(self.pane);
        }
    }
}

fn text(value: Option<&CefString>) -> String {
    value.map(ToString::to_string).unwrap_or_default()
}

/// The pointer for CEF's cursor type.
fn cursor(kind: CursorType) -> Cursor {
    match kind {
        CursorType::HAND => Cursor::Pointer,
        CursorType::IBEAM | CursorType::VERTICALTEXT => Cursor::Text,
        CursorType::WAIT | CursorType::PROGRESS => Cursor::Wait,
        CursorType::CROSS | CursorType::CELL => Cursor::Crosshair,
        CursorType::NOTALLOWED | CursorType::NODROP => Cursor::NotAllowed,
        CursorType::GRAB => Cursor::Grab,
        CursorType::GRABBING => Cursor::Grabbing,
        CursorType::EASTRESIZE
        | CursorType::WESTRESIZE
        | CursorType::EASTWESTRESIZE
        | CursorType::COLUMNRESIZE => Cursor::ResizeColumn,
        CursorType::NORTHRESIZE
        | CursorType::SOUTHRESIZE
        | CursorType::NORTHSOUTHRESIZE
        | CursorType::ROWRESIZE => Cursor::ResizeRow,
        CursorType::NONE => Cursor::None,
        _ => Cursor::Arrow,
    }
}

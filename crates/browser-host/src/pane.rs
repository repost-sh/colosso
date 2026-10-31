//! One browser pane's CEF client. Every handler carries the pane id and
//! reports to [`crate::host`], which owns the pane's state on the UI thread.
use crate::host;
use cef::{
    AcceleratedPaintInfo, Browser, BrowserSettings, CefString, Client, ContextMenuHandler,
    ContextMenuParams, CursorInfo, CursorType, DialogHandler, DictionaryValue, DisplayHandler,
    FindHandler, Frame, ImplClient, ImplContextMenuHandler, ImplDisplayHandler, ImplFindHandler,
    ImplLifeSpanHandler, ImplLoadHandler, ImplRenderHandler, JsdialogHandler, LifeSpanHandler,
    LoadHandler, MenuModel, PaintElementType, PermissionHandler, PopupFeatures, Rect,
    RenderHandler, RequestHandler, RunContextMenuCallback, ScreenInfo, WindowInfo,
    WindowOpenDisposition, WrapClient, WrapContextMenuHandler, WrapDisplayHandler, WrapFindHandler,
    WrapLifeSpanHandler, WrapLoadHandler, WrapRenderHandler, rc::Rc, wrap_client,
    wrap_context_menu_handler, wrap_display_handler, wrap_find_handler, wrap_life_span_handler,
    wrap_load_handler, wrap_render_handler,
};
use colosso_browser_ipc::{Cursor, HostEvent, PaneId};
use std::os::raw::c_int;

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
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(PaneRender::new(self.pane))
        }
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

wrap_render_handler! {
    struct PaneRender {
        pane: PaneId,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let (width, height) = host::logical_size(self.pane);
                *rect = Rect { x: 0, y: 0, width, height };
            }
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> c_int {
            let Some(info) = screen_info else {
                return 0;
            };
            let (width, height) = host::logical_size(self.pane);
            info.device_scale_factor = host::scale(self.pane);
            info.rect = Rect { x: 0, y: 0, width, height };
            info.available_rect = info.rect.clone();
            1
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            _type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            _buffer: *const u8,
            _width: c_int,
            _height: c_int,
        ) {
            // Shared textures are on; CPU frames only come when the GPU
            // process fell back, which the app reports as a blank pane.
            host::software_frame(self.pane);
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            let layer = if type_ == PaintElementType::POPUP {
                crate::surfaces::Layer::Popup
            } else {
                crate::surfaces::Layer::Page
            };
            if let Some(info) = info {
                host::accelerated_frame(self.pane, layer, info);
            }
        }

        fn on_popup_show(&self, _browser: Option<&mut Browser>, show: c_int) {
            host::popup_shown(self.pane, show != 0);
        }

        fn on_popup_size(&self, _browser: Option<&mut Browser>, rect: Option<&Rect>) {
            if let Some(rect) = rect {
                host::popup_moved(self.pane, rect);
            }
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
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            use cef::ImplFrame;
            if frame.is_some_and(|f| f.is_main() == 1) {
                host::page_changed(self.pane, |page| {
                    page.url = url.map(ToString::to_string).unwrap_or_default();
                });
            }
        }

        fn on_loading_progress_change(&self, _browser: Option<&mut Browser>, progress: f64) {
            // Whole percents, so a load sends a hundred changes at most.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0 through 100 first"
            )]
            let percent = (progress.clamp(0., 1.) * 100.).round() as u8;
            host::page_changed(self.pane, |page| page.progress = percent);
        }

        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            host::page_changed(self.pane, |page| {
                page.title = title.map(ToString::to_string).unwrap_or_default();
            });
        }

        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: *mut u8,
            type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> c_int {
            host::cursor_changed(self.pane, cursor(type_));
            // The app draws the pointer; CEF must not touch NSCursor.
            1
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
            _browser: Option<&mut Browser>,
            is_loading: c_int,
            can_go_back: c_int,
            can_go_forward: c_int,
        ) {
            host::page_changed(self.pane, |page| {
                if is_loading != 0 && !page.loading {
                    page.progress = 0;
                }
                page.loading = is_loading != 0;
                page.can_go_back = can_go_back != 0;
                page.can_go_forward = can_go_forward != 0;
            });
            host::loading_changed(self.pane, is_loading != 0);
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
            _popup_id: c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut c_int>,
        ) -> c_int {
            // No native popup windows: the page opens in its own pane, and
            // a link from DevTools in a new tab of the app.
            match target_url {
                Some(url) if self.devtools => host::send(&HostEvent::OpenUrl {
                    pane: self.pane,
                    url: url.to_string(),
                }),
                Some(url) => host::navigate_later(self.pane, url.to_string()),
                None => {}
            }
            1
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

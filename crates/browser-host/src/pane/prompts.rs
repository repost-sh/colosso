//! The handlers that put a page's questions to the person, through the app: sign-ins and
//! certificate errors, script dialogs, file choosers and permission prompts.
use super::text;
use crate::dialogs::{self, Waiting};
use crate::host;
use cef::{
    AuthCallback, Browser, Callback, CefString, CefStringList, DialogHandler, Errorcode,
    FileDialogCallback, FileDialogMode, Frame, ImplDialogHandler, ImplJsdialogHandler,
    ImplPermissionHandler, ImplRequestHandler, JsdialogCallback, JsdialogHandler, JsdialogType,
    MediaAccessCallback, PermissionHandler, PermissionPromptCallback, PermissionRequestResult,
    RequestHandler, Sslinfo, TerminationStatus, WrapDialogHandler, WrapJsdialogHandler,
    WrapPermissionHandler, WrapRequestHandler, rc::Rc, wrap_dialog_handler, wrap_jsdialog_handler,
    wrap_permission_handler, wrap_request_handler,
};
use colosso_browser_ipc::{Dialog, PaneId};
use std::os::raw::c_int;

/// The handler for `pane`'s network requests: sign-ins, certificate errors, a renderer gone.
pub(super) fn requests(pane: PaneId) -> RequestHandler {
    PaneRequests::new(pane)
}

/// The handler for `pane`'s script dialogs: alerts, confirms, prompts, leaving the page.
pub(super) fn script_dialogs(pane: PaneId) -> JsdialogHandler {
    PaneScriptDialogs::new(pane)
}

/// The handler for `pane`'s file choosers.
pub(super) fn files(pane: PaneId) -> DialogHandler {
    PaneFiles::new(pane)
}

/// The handler for what `pane`'s pages ask to use: camera, microphone, location and more.
pub(super) fn permissions(pane: PaneId) -> PermissionHandler {
    PanePermissions::new(pane)
}

wrap_request_handler! {
    struct PaneRequests {
        pane: PaneId,
    }

    impl RequestHandler {
        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            status: TerminationStatus,
            error_code: c_int,
            _error_string: Option<&CefString>,
        ) {
            host::renderer_gone(self.pane, format!("{status:?} ({error_code})"));
        }

        // Called on the IO thread.
        fn auth_credentials(
            &self,
            _browser: Option<&mut Browser>,
            origin_url: Option<&CefString>,
            is_proxy: c_int,
            host: Option<&CefString>,
            port: c_int,
            realm: Option<&CefString>,
            _scheme: Option<&CefString>,
            callback: Option<&mut AuthCallback>,
        ) -> c_int {
            let Some(callback) = callback else {
                return 0;
            };
            let host = text(host);
            let host = if matches!(port, 80 | 443) || port <= 0 {
                host
            } else {
                format!("{host}:{port}")
            };
            dialogs::ask(
                self.pane,
                &text(origin_url),
                Dialog::Auth {
                    host,
                    realm: text(realm),
                    proxy: is_proxy != 0,
                },
                Waiting::Auth(callback.clone()),
            );
            1
        }

        fn on_certificate_error(
            &self,
            _browser: Option<&mut Browser>,
            cert_error: Errorcode,
            request_url: Option<&CefString>,
            _ssl_info: Option<&mut Sslinfo>,
            callback: Option<&mut Callback>,
        ) -> c_int {
            let Some(callback) = callback else {
                return 0;
            };
            dialogs::ask(
                self.pane,
                &text(request_url),
                Dialog::Certificate {
                    error: dialogs::certificate_error(cert_error),
                },
                Waiting::Certificate(callback.clone()),
            );
            1
        }
    }
}

wrap_jsdialog_handler! {
    struct PaneScriptDialogs {
        pane: PaneId,
    }

    impl JsdialogHandler {
        fn on_jsdialog(
            &self,
            _browser: Option<&mut Browser>,
            origin_url: Option<&CefString>,
            dialog_type: JsdialogType,
            message_text: Option<&CefString>,
            default_prompt_text: Option<&CefString>,
            callback: Option<&mut JsdialogCallback>,
            _suppress_message: Option<&mut c_int>,
        ) -> c_int {
            let Some(callback) = callback else {
                return 0;
            };
            let message = text(message_text);
            let question = if dialog_type == JsdialogType::CONFIRM {
                Dialog::Confirm { message }
            } else if dialog_type == JsdialogType::PROMPT {
                Dialog::Prompt {
                    message,
                    default_text: text(default_prompt_text),
                }
            } else {
                Dialog::Alert { message }
            };
            dialogs::ask(
                self.pane,
                &text(origin_url),
                question,
                Waiting::Script(callback.clone()),
            );
            1
        }

        fn on_before_unload_dialog(
            &self,
            browser: Option<&mut Browser>,
            _message_text: Option<&CefString>,
            is_reload: c_int,
            callback: Option<&mut JsdialogCallback>,
        ) -> c_int {
            use cef::{ImplBrowser, ImplFrame};
            let Some(callback) = callback else {
                return 0;
            };
            // Pages no longer choose the text; the origin is the main frame's.
            let origin = browser
                .and_then(|b| b.main_frame())
                .map(|f| CefString::from(&f.url()).to_string())
                .unwrap_or_default();
            dialogs::ask(
                self.pane,
                &origin,
                Dialog::BeforeUnload {
                    reload: is_reload != 0,
                },
                Waiting::Script(callback.clone()),
            );
            1
        }

        fn on_reset_dialog_state(&self, _browser: Option<&mut Browser>) {
            dialogs::reset(self.pane);
        }
    }
}

wrap_dialog_handler! {
    struct PaneFiles {
        pane: PaneId,
    }

    impl DialogHandler {
        fn on_file_dialog(
            &self,
            browser: Option<&mut Browser>,
            mode: FileDialogMode,
            title: Option<&CefString>,
            default_file_path: Option<&CefString>,
            accept_filters: Option<&mut CefStringList>,
            _accept_extensions: Option<&mut CefStringList>,
            _accept_descriptions: Option<&mut CefStringList>,
            callback: Option<&mut FileDialogCallback>,
        ) -> c_int {
            use cef::{ImplBrowser, ImplFrame};
            let Some(callback) = callback else {
                return 0;
            };
            let origin = browser
                .and_then(|b| b.main_frame())
                .map(|f| CefString::from(&f.url()).to_string())
                .unwrap_or_default();
            dialogs::ask(
                self.pane,
                &origin,
                Dialog::File {
                    mode: dialogs::file_mode(mode),
                    title: text(title),
                    default_path: text(default_file_path),
                    accept: dialogs::strings(accept_filters),
                },
                Waiting::File(callback.clone()),
            );
            1
        }
    }
}

wrap_permission_handler! {
    struct PanePermissions {
        pane: PaneId,
    }

    impl PermissionHandler {
        fn on_request_media_access_permission(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> c_int {
            let Some(callback) = callback else {
                return 0;
            };
            dialogs::ask(
                self.pane,
                &text(requesting_origin),
                Dialog::Permission {
                    permissions: dialogs::media(requested_permissions),
                },
                Waiting::Media(requested_permissions, callback.clone()),
            );
            1
        }

        fn on_show_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            prompt_id: u64,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut PermissionPromptCallback>,
        ) -> c_int {
            let Some(callback) = callback else {
                return 0;
            };
            dialogs::ask(
                self.pane,
                &text(requesting_origin),
                Dialog::Permission {
                    permissions: dialogs::permissions(requested_permissions),
                },
                Waiting::Prompt(prompt_id, callback.clone()),
            );
            1
        }

        fn on_dismiss_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            prompt_id: u64,
            _result: PermissionRequestResult,
        ) {
            dialogs::prompt_dismissed(self.pane, prompt_id);
        }
    }
}

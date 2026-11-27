//! The CEF application object, shared by the browser process and helpers.
use cef::{
    App, Browser, BrowserProcessHandler, CefString, CommandLine, Frame, ImplApp,
    ImplBrowserProcessHandler, ImplCommandLine, ImplLoadHandler, ImplRenderProcessHandler,
    LoadHandler, ProcessId, ProcessMessage, RenderProcessHandler, TransitionType, V8Context,
    WrapApp, WrapBrowserProcessHandler, WrapLoadHandler, WrapRenderProcessHandler, rc::Rc,
    wrap_app, wrap_browser_process_handler, wrap_load_handler, wrap_render_process_handler,
};
use std::os::raw::c_int;

mod switches;

wrap_app! {
    pub(crate) struct HostApp;

    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let (None, Some(command_line)) = (process_type, command_line) else {
                return;
            };
            switches::browser(command_line, crate::embedded::data_folder());
            if crate::mock_keychain() {
                command_line.append_switch(Some(&"use-mock-keychain".into()));
            }
            if let Some(ends) = crate::extensions::pipe_switch() {
                command_line.append_switch_with_value(
                    Some(&"remote-debugging-io-pipes".into()),
                    Some(&ends.as_str().into()),
                );
            }
            let extra = if cfg!(debug_assertions) {
                std::env::var("KOYO_NATIVE_SWITCHES").unwrap_or_default()
            } else {
                String::new()
            };
            for switch in extra.split_whitespace() {
                match switch.split_once('=') {
                    Some((name, value)) => command_line
                        .append_switch_with_value(Some(&name.into()), Some(&value.into())),
                    None => command_line.append_switch(Some(&switch.into())),
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(HostProcessHandler::new())
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(PageScripts::new())
        }
    }
}

wrap_render_process_handler! {
    struct PageScripts;

    impl RenderProcessHandler {
        fn on_context_created(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            if let (Some(frame), Some(context)) = (frame, context) {
                crate::session::renderer::context_created(frame, context);
                crate::tone::renderer::context_created(frame, context);
                crate::sight::renderer::context_created(frame, context);
                crate::passkeys::renderer::context_created(frame, context);
            }
        }

        fn on_context_released(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            if let Some(context) = context {
                crate::passkeys::renderer::context_released(context);
            }
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(PageLoads::new())
        }

        fn on_process_message_received(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> c_int {
            message.map_or(0, |message| c_int::from(crate::passkeys::renderer::answered(message)))
        }
    }
}

wrap_browser_process_handler! {
    struct HostProcessHandler;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            crate::host::context_initialized();
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            if crate::embedded::engine().is_some() {
                crate::embedded::pump::schedule(delay_ms);
            }
        }

        fn default_client(&self) -> Option<cef::Client> {
            Some(crate::native::stray_client())
        }
    }
}

wrap_load_handler! {
    struct PageLoads;

    impl LoadHandler {
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            if let Some(frame) = frame {
                crate::passkeys::renderer::load_started(frame);
            }
        }
    }
}

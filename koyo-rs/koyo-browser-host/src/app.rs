//! The CEF application object, shared by the browser process and helpers.
use cef::{
    App, Browser, BrowserProcessHandler, CefString, CommandLine, Frame, ImplApp,
    ImplBrowserProcessHandler, ImplCommandLine, ImplLoadHandler, ImplRenderProcessHandler,
    LoadHandler, ProcessId, ProcessMessage, RenderProcessHandler, TransitionType, V8Context,
    WrapApp, WrapBrowserProcessHandler, WrapLoadHandler, WrapRenderProcessHandler, rc::Rc,
    wrap_app, wrap_browser_process_handler, wrap_load_handler, wrap_render_process_handler,
};
use std::os::raw::c_int;

wrap_app! {
    pub(crate) struct HostApp;

    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // Only the browser process decides; helpers inherit its switches.
            let (None, Some(command_line)) = (process_type, command_line) else {
                return;
            };
            // Panes are composed by Koyo; Chromium never owns a window,
            // a dock icon or the menu bar.
            for switch in [
                "disable-background-mode",
                "no-default-browser-check",
                "no-first-run",
                // Sign-ins reach the request handler, which asks in the
                // pane, instead of Chrome's own prompt, which has no window
                // to show in.
                "disable-chrome-login-prompt",
            ] {
                command_line.append_switch(Some(&switch.into()));
            }
            // Ad-hoc local builds would ask for Keychain access after every
            // rebuild; their profiles use a fixed cookie key instead.
            if crate::mock_keychain() {
                command_line.append_switch(Some(&"use-mock-keychain".into()));
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

// A renderer's hold on the frames' scripts: the passkey bridge in each frame
// (`passkeys/renderer.rs`).
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
    }
}

// A renderer's hearing of a frame's document starting to load: the passkey bridge goes in
// then, before the page's first script.
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

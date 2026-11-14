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
            // In the app's process pages are Chrome's own browsers, whose windows are the
            // host's to make: Chrome opens none as it starts and offers to bring none back.
            if crate::host::native() {
                for switch in [
                    "no-startup-window",
                    "disable-session-crashed-bubble",
                    "hide-crash-restore-bubble",
                    // Whether a page is on screen is the app's to say, by where its views are
                    // (`native.rs`): a page an agent drives stays awake in a window macOS
                    // counts as covered, as its parked holder is whenever Koyo is behind.
                    "disable-backgrounding-occluded-windows",
                ] {
                    command_line.append_switch(Some(&switch.into()));
                }
                // Switches to try things out with, in a debug build:
                // `KOYO_NATIVE_SWITCHES="a b=c"`.
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

        // In the app's process the app runs the loop: CEF says when it next has work.
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            #[cfg(target_os = "macos")]
            if crate::host::native() {
                crate::embedded::pump::schedule(delay_ms);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = delay_ms;
        }

        // A browser Chrome makes on its own, which only a Chrome-style engine does.
        fn default_client(&self) -> Option<cef::Client> {
            #[cfg(target_os = "macos")]
            if crate::host::native() {
                return Some(crate::native::stray_client());
            }
            None
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

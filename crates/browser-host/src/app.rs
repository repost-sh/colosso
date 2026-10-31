//! The CEF application object, shared by the browser process and helpers.
use cef::{
    App, BrowserProcessHandler, CefString, CommandLine, ImplApp, ImplBrowserProcessHandler,
    ImplCommandLine, WrapApp, WrapBrowserProcessHandler, rc::Rc, wrap_app,
    wrap_browser_process_handler,
};

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
            // Panes are composed by Colosso; Chromium never owns a window,
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

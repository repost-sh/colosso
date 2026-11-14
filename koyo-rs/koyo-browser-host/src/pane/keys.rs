//! Keys a native page leaves unhandled, handed back to the app: its own shortcuts go on
//! working while the keyboard is in a page (`native.rs`). An off-screen page's keys come from
//! the app in the first place, which keeps what it wants, so only a native page has this.

use cef::{
    Browser, ImplKeyboardHandler, KeyEvent, KeyEventType, KeyboardHandler, WrapKeyboardHandler,
    rc::Rc, wrap_keyboard_handler,
};
use koyo_browser_ipc::PaneId;
use std::os::raw::c_int;

pub(super) fn keys(pane: PaneId) -> KeyboardHandler {
    PaneKeys::new(pane)
}

wrap_keyboard_handler! {
    struct PaneKeys {
        pane: PaneId,
    }

    impl KeyboardHandler {
        fn on_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            os_event: *mut u8,
        ) -> c_int {
            let down = event.is_some_and(|event| {
                event.type_ == KeyEventType::RAWKEYDOWN || event.type_ == KeyEventType::KEYDOWN
            });
            #[cfg(target_os = "macos")]
            if down {
                return c_int::from(crate::native::unhandled_key(self.pane, os_event.cast()));
            }
            let _ = (down, os_event);
            0
        }
    }
}

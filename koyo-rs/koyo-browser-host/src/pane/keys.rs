//! Keys a page leaves unhandled, handed back to the app: its own shortcuts go on working
//! while the keyboard is in a page (`native.rs`). And what's done with a key before the page
//! hears it: the person counted as at the page, Escape out of full screen. A page's keys are
//! `AppKit`'s to hand it, so the app sees none of them but through here.

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
        // A key the person pressed, before the page hears it: the app is told the person is
        // at the page, as a press on it tells it. A key an agent sends has no event of the
        // system's behind it.
        fn on_pre_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            os_event: *mut u8,
            _is_keyboard_shortcut: Option<&mut c_int>,
        ) -> c_int {
            let down = event.filter(|event| event.type_ == KeyEventType::RAWKEYDOWN);
            if down.is_some() && !os_event.is_null() && due(self.pane) {
                crate::host::send(&koyo_browser_ipc::HostEvent::Typed { pane: self.pane });
            }
            // Escape takes a page out of full screen, as in any browser, before the page hears
            // it: Chromium's own does so only in a window of its own that has the keys.
            let escape = down.is_some_and(|event| event.windows_key_code == ESCAPE);
            c_int::from(escape && crate::host::leave_fullscreen(self.pane))
        }

        fn on_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            os_event: *mut u8,
        ) -> c_int {
            let down = event.is_some_and(|event| {
                event.type_ == KeyEventType::RAWKEYDOWN || event.type_ == KeyEventType::KEYDOWN
            });
            if down {
                return c_int::from(crate::native::unhandled_key(self.pane, os_event.cast()));
            }
            0
        }
    }
}

/// Escape's virtual key code, as Chromium names keys.
const ESCAPE: c_int = 0x1B;

/// How often the app is told of the person's keys in a pane, at most.
const TELL_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

thread_local! {
    /// When the app was last told of keys in each pane.
    static TOLD: std::cell::RefCell<std::collections::HashMap<PaneId, std::time::Instant>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Whether the app is due to hear of a key in `pane`, which then counts as told.
fn due(pane: PaneId) -> bool {
    let now = std::time::Instant::now();
    TOLD.with(|told| {
        let mut told = told.borrow_mut();
        // Panes long closed needn't be kept.
        told.retain(|_, at| now.duration_since(*at) < std::time::Duration::from_secs(60));
        let due = told.get(&pane).is_none_or(|at| now.duration_since(*at) >= TELL_EVERY);
        if due {
            told.insert(pane, now);
        }
        due
    })
}

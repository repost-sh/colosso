//! The windows of the app's own that float over its main one (a menu's or a popover's panel,
//! a dialog, the launcher, the address, the tab switcher), as `AppKit` has them: the level they
//! sit at, the Space they're on, and when they leave the screen (`docs/native-layer.md` 7).
//! The layer is the one writer of these.
//!
//! A native menu, and a window of Chromium's own, always show above the app's panels, and a
//! panel that hands over to one is off the screen before it opens. GPUI gives a floating window
//! the very level `AppKit` gives a menu's, where only the order they were shown in decides
//! which is in front: a menu opened for the main window while a popover was still up came up
//! behind the popover (the owner, 2026-11-17).

/// `NSPopUpMenuWindowLevel`: where `AppKit` puts a menu's window.
pub const MENUS: isize = 101;
/// `NSNormalWindowLevel`: the app's main windows.
pub const MAIN: isize = 0;
/// Where a native page's holder sits, a level under the main windows (the browser host).
pub const HOLDERS: isize = -1;

/// The level of the app's floating windows: over its main windows and everything of other
/// apps' a pop-up panel is over, and one under the menus', so a menu is in front of every one
/// of them whatever opened it and in whatever order they were shown.
#[must_use]
pub const fn panel_level() -> isize {
    MENUS - 1
}

#[cfg(target_os = "macos")]
pub use appkit::{dismiss, float};

#[cfg(target_os = "macos")]
mod appkit {
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};

    /// `NSWindowCollectionBehaviorManaged` and `NSWindowCollectionBehaviorFullScreenAuxiliary`.
    const MANAGED: usize = 1 << 2;
    const FULL_SCREEN_AUXILIARY: usize = 1 << 8;

    /// Makes `window` one of the app's floating windows: at their level
    /// ([`super::panel_level`]), kept to the Space it opens on (GPUI has a pop-up panel join
    /// every Space, so a menu left open went with the person to another desktop), still shown
    /// over a full-screen window of the app's, and unseen while the app isn't in front, coming
    /// back with it.
    ///
    /// # Safety
    /// `window` is null or a live `NSWindow`, and this is the main thread.
    pub unsafe fn float(window: *mut AnyObject) {
        let Some(window) = (unsafe { window.as_ref() }) else {
            return;
        };
        // SAFETY: a live window, three of its settings set as `AppKit` declares them.
        unsafe {
            let _: () = msg_send![window, setLevel: super::panel_level()];
            let _: () = msg_send![window, setCollectionBehavior: MANAGED | FULL_SCREEN_AUXILIARY];
            let _: () = msg_send![window, setHidesOnDeactivate: Bool::YES];
        }
    }

    /// Takes `window`, a floating window that's closing, off the screen now. GPUI closes a
    /// window once the update that asked for it has unwound, and what the close makes way for
    /// (a menu, a page's own pop-up) can open before that, under a panel that's as good as
    /// gone.
    ///
    /// # Safety
    /// `window` is null or a live `NSWindow`, and this is the main thread.
    pub unsafe fn dismiss(window: *mut AnyObject) {
        let Some(window) = (unsafe { window.as_ref() }) else {
            return;
        };
        // SAFETY: a live window, ordered out as `AppKit` orders one out.
        let _: () = unsafe { msg_send![window, orderOut: std::ptr::null::<AnyObject>()] };
    }
}

#[cfg(test)]
mod tests {
    use super::{HOLDERS, MAIN, MENUS, panel_level};

    #[test]
    fn a_menu_is_in_front_of_every_panel_by_its_level_alone() {
        assert!(panel_level() < MENUS);
    }

    #[test]
    fn a_panel_is_over_the_main_windows_and_the_pages_in_them() {
        assert!(panel_level() > MAIN);
        assert!(panel_level() > HOLDERS);
    }

    #[test]
    fn a_panel_stays_where_a_pop_up_panel_is_among_other_apps_windows() {
        // Above the Dock (20), the menu bar (24) and status items (25), as before.
        assert!(panel_level() > 25);
    }
}

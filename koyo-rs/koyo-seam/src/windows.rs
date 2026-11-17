//! The windows of the app's own that float over its main one (a menu's or a popover's panel,
//! a dialog, the launcher, the address, the tab switcher), as `AppKit` has them: the level they
//! sit at, the Space they're on, and when they leave the screen (`docs/native-layer.md` 7).
//! The layer is the one writer of these.
//!
//! A native menu, and a window of Chromium's own, always show above the app's panels, and a
//! panel that hands over to one is off the screen before it opens. GPUI gives a floating window
//! the very level `AppKit` gives a menu's, where only the order they were shown in decides
//! which is in front: a menu opened for the main window while a popover was still up came up
//! behind the popover (the owner, 2026-11-17). And a window of Chromium's own belongs to the
//! app's window its page is drawn in, not to the page's holder under it ([`adopt`]).

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

/// What to do with a window of Chromium's own for it to belong to the app's window its page
/// is drawn in: by the window it's a child of now, if any, and the one it should be a child of.
/// Windows are told apart by their addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adoption {
    /// It's that window's child already.
    Kept,
    /// It's nobody's child: it's made that window's.
    Taken,
    /// It's another window's child, a page's holder's: it's taken from that one first.
    TakenFrom(usize),
}

/// How a window whose parent is `parent` (0 for none) comes to be `over`'s child.
#[must_use]
pub const fn adoption(parent: usize, over: usize) -> Adoption {
    if parent == over {
        Adoption::Kept
    } else if parent == 0 {
        Adoption::Taken
    } else {
        Adoption::TakenFrom(parent)
    }
}

#[cfg(target_os = "macos")]
pub use appkit::{adopt, dismiss, float};

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

    /// Makes `window`, a window of Chromium's own that's being shown (an extension's pop-up),
    /// a child of `over`, the app's window its page is drawn in: it's in front of that window,
    /// on its Space, and moves with it. Chromium ties such a window to the page's holder, which
    /// sits a level under the app's windows, so it opened behind the app's window, unseen, with
    /// the keyboard (the owner, 2026-11-17). Asked again, as when Chromium has tied it to the
    /// holder again, it's taken back; already `over`'s, nothing is done. Told what it did.
    ///
    /// `AppKit` shows a window as it's made a child: this is for a window that's to be seen.
    ///
    /// # Safety
    /// `window` and `over` are each null or a live `NSWindow`, and this is the main thread.
    pub unsafe fn adopt(window: *mut AnyObject, over: *mut AnyObject) -> Option<super::Adoption> {
        /// `NSWindowAbove`.
        const ABOVE: isize = 1;
        let (window, over) = unsafe { (window.as_ref()?, over.as_ref()?) };
        if std::ptr::eq(window, over) {
            return None;
        }
        // SAFETY: live windows; a child window read, removed and added as `AppKit` declares.
        unsafe {
            let parent: *mut AnyObject = msg_send![window, parentWindow];
            let how = super::adoption(parent as usize, std::ptr::from_ref(over) as usize);
            if let super::Adoption::TakenFrom(_) = how {
                let _: () = msg_send![parent, removeChildWindow: window];
            }
            if how != super::Adoption::Kept {
                let _: () = msg_send![over, addChildWindow: window, ordered: ABOVE];
            }
            Some(how)
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
    use super::{Adoption, HOLDERS, MAIN, MENUS, adoption, panel_level};

    #[test]
    fn a_window_of_chromiums_is_taken_from_the_holder_once_and_left_once_its_the_apps() {
        let (holder, main) = (0x1000, 0x2000);
        assert_eq!(adoption(holder, main), Adoption::TakenFrom(holder));
        assert_eq!(adoption(0, main), Adoption::Taken);
        assert_eq!(adoption(main, main), Adoption::Kept, "asked again, nothing is done");
        assert_eq!(adoption(holder, main), Adoption::TakenFrom(holder));
    }

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
        assert!(panel_level() > 25);
    }
}

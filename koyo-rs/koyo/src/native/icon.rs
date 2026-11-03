//! The app's icon in the Dock, from `icons/icon.png` (made by `script/icon`). A bundle shows
//! its `Koyo.icns` before the app starts; a build run outside one, as `script/dev` runs it,
//! would show the system's generic icon without this.

use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::NSData;

const ICON: &[u8] = include_bytes!("../../icons/icon.png");

pub fn set_dock_icon() {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        // SAFETY: on the main thread, as AppKit wants, with an image that outlives the call.
        unsafe { NSApplication::sharedApplication(main).setApplicationIconImage(Some(&image)) };
    }
}

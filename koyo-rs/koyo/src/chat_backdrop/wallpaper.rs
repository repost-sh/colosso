//! Each screen's wallpaper as `NSWorkspace` names it: its file, how it's laid out, and the
//! colour round a picture that doesn't cover the screen. Read on the main thread, as `AppKit`
//! asks of its screens; a few calls that cost next to nothing.

use std::path::PathBuf;
use std::time::SystemTime;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSColor, NSColorSpace, NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey,
    NSWorkspaceDesktopImageFillColorKey, NSWorkspaceDesktopImageScalingKey,
};
use objc2_foundation::{NSNumber, NSString};

use super::layout::Scaling;

/// A screen's wallpaper: what its picture is made from, as far as `AppKit` says it.
#[derive(Clone, Debug, PartialEq)]
pub struct Wallpaper {
    pub path: PathBuf,
    /// When the file last changed, so one saved over under the same name is read again.
    pub modified: Option<SystemTime>,
    pub scaling: Scaling,
    /// The colour round a picture that leaves part of the screen bare, in sRGB.
    pub fill: [f32; 3],
}

/// The colour round a picture when `AppKit` says none.
const FILL: [f32; 3] = [0., 0., 0.];

/// The wallpaper of the screen whose display ID is `display`, or none when it has no file
/// `AppKit` names.
pub fn read(display: u32, main: MainThreadMarker) -> Option<Wallpaper> {
    let screen = screen(display, main)?;
    let workspace = NSWorkspace::sharedWorkspace();
    let path = workspace.desktopImageURLForScreen(&screen)?.to_file_path()?;
    let modified = std::fs::metadata(&path).and_then(|file| file.modified()).ok();
    let options = workspace.desktopImageOptionsForScreen(&screen);
    let option = |key: &NSString| options.as_ref().and_then(|options| options.objectForKey(key));
    // SAFETY: constants `AppKit` defines.
    let (scaling, clipping, fill) = unsafe {
        (
            option(NSWorkspaceDesktopImageScalingKey),
            option(NSWorkspaceDesktopImageAllowClippingKey),
            option(NSWorkspaceDesktopImageFillColorKey),
        )
    };
    let number = |value: Option<Retained<objc2::runtime::AnyObject>>| {
        value.and_then(|value| value.downcast::<NSNumber>().ok())
    };
    // Unsaid, a picture fills its screen, as System Settings sets one.
    let scaling = number(scaling).map_or(3, |scaling| scaling.unsignedIntegerValue());
    let clipping = number(clipping).is_none_or(|clipping| clipping.boolValue());
    let fill = fill
        .and_then(|fill| fill.downcast::<NSColor>().ok())
        .and_then(|fill| srgb(&fill))
        .unwrap_or(FILL);
    Some(Wallpaper {
        path,
        modified,
        scaling: Scaling::from_options(scaling, clipping),
        fill,
    })
}

/// The screen whose display ID is `display`.
fn screen(display: u32, main: MainThreadMarker) -> Option<Retained<NSScreen>> {
    NSScreen::screens(main)
        .iter()
        .find(|screen| number(screen) == Some(display))
}

/// The display ID of `screen`, as its `NSScreenNumber`, which GPUI's display IDs are on macOS.
fn number(screen: &NSScreen) -> Option<u32> {
    let key = NSString::from_str("NSScreenNumber");
    let value = screen.deviceDescription().objectForKey(&key)?;
    Some(value.downcast::<NSNumber>().ok()?.unsignedIntValue())
}

/// `colour` in sRGB, from 0 to 1, if it's one that converts.
fn srgb(colour: &NSColor) -> Option<[f32; 3]> {
    let colour = colour.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let channel = |value: f64| value.clamp(0., 1.) as f32;
    Some([
        channel(colour.redComponent()),
        channel(colour.greenComponent()),
        channel(colour.blueComponent()),
    ])
}

//! Each screen's wallpaper as `NSWorkspace` names it: its file, how it's laid out, and the
//! colour round a picture that doesn't cover the screen. Read on the main thread, as `AppKit`
//! asks of its screens; a few calls that cost next to nothing.

use std::path::PathBuf;
use std::time::SystemTime;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSColor, NSColorSpace, NSScreen, NSWorkspace,
    NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageFillColorKey,
    NSWorkspaceDesktopImageScalingKey,
};
use objc2_foundation::{NSNumber, NSString, ns_string};

use super::layout::Scaling;

/// A screen's wallpaper: what its picture is made from, as far as `AppKit` says it.
#[derive(Clone, Debug, PartialEq)]
pub struct Wallpaper {
    pub path: PathBuf,
    /// When the file last changed, so one saved over under the same name is read again.
    pub modified: Option<SystemTime>,
    pub scaling: Scaling,
    /// Which of the file's pictures to show, for a dynamic wallpaper: the one macOS shows now.
    pub image: Option<usize>,
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
    let path = workspace
        .desktopImageURLForScreen(&screen)?
        .to_file_path()?;
    let modified = std::fs::metadata(&path)
        .and_then(|file| file.modified())
        .ok();
    let options = workspace.desktopImageOptionsForScreen(&screen);
    let option = |key: &NSString| {
        options
            .as_ref()
            .and_then(|options| options.objectForKey(key))
    };
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
    let scaling = number(scaling).map_or(3, |scaling| scaling.unsignedIntegerValue());
    let clipping = number(clipping).is_none_or(|clipping| clipping.boolValue());
    let fill = fill
        .and_then(|fill| fill.downcast::<NSColor>().ok())
        .and_then(|fill| srgb(&fill))
        .unwrap_or(FILL);
    Some(Wallpaper {
        image: super::dynamic::image(&path, modified),
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
    let value = (screen.deviceDescription()).objectForKey(ns_string!("NSScreenNumber"))?;
    Some(value.downcast::<NSNumber>().ok()?.unsignedIntValue())
}

/// The size in pixels of each screen the app's visible windows are on, once each.
pub fn screens_of_windows(main: MainThreadMarker) -> Vec<(u32, u32)> {
    let mut seen = Vec::new();
    let mut sizes = Vec::new();
    for window in NSApplication::sharedApplication(main).windows() {
        let Some(screen) = window.screen().filter(|_| window.isVisible()) else {
            continue;
        };
        let Some(display) = number(&screen).filter(|display| !seen.contains(display)) else {
            continue;
        };
        seen.push(display);
        let (size, scale) = (screen.frame().size, screen.backingScaleFactor());
        let pixels = |points: f64| (points * scale).round() as u32;
        sizes.push((pixels(size.width), pixels(size.height)));
    }
    sizes
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

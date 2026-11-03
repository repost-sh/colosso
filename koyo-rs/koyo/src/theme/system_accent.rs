//! The accent colour System Settings › Appearance sets, read from `AppKit` on the main thread
//! under the light and the dark appearance, and read again whenever macOS says the system's
//! colours changed. "Multicolor" reads as the default blue.

use std::cell::Cell;
use std::ptr::NonNull;

use block2::{RcBlock, StackBlock};
use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::{App, Hsla};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAppearance, NSAppearanceName, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor,
    NSColorSpace, NSSystemColorsDidChangeNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};

use super::accent::{self, Accents};

/// Takes up the system's accent, and each change to it from now on. Off the main thread,
/// where `AppKit` isn't to be asked and where tests run, the app keeps its own accent.
pub fn follow(cx: &mut App) {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    take_up(main, cx);
    let (tell, mut told) = mpsc::unbounded();
    let changed = RcBlock::new(move |_: NonNull<NSNotification>| {
        // Unheard only once the app has stopped listening, on its way out.
        tell.unbounded_send(()).ok();
    });
    // SAFETY: the name is AppKit's own constant, and the block only sends on a channel,
    // which any thread may do; the main queue runs it on the main thread regardless.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSSystemColorsDidChangeNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &changed,
        )
    };
    cx.spawn(async move |cx| {
        // Kept for as long as the app listens.
        let _observer = observer;
        while told.next().await.is_some() {
            cx.update(|cx| take_up(main, cx));
        }
    })
    .detach();
}

/// Reads the accent and puts it in the palette and gpui-kit's theme. The notification also
/// comes for system colours other than the accent, which leave the accent as it was and so
/// repaint nothing.
fn take_up(main: MainThreadMarker, cx: &mut App) {
    match read(main) {
        Some(accents) => {
            log::info!(
                "system accent: {} light, {} dark",
                accent::hex(accents.light),
                accent::hex(accents.dark)
            );
            accent::set(accents, cx);
        }
        None => log::warn!("couldn't read the system accent; keeping the one in use"),
    }
}

/// The accent under each appearance. Only the main thread may draw, so only it may ask.
fn read(_main: MainThreadMarker) -> Option<Accents> {
    // SAFETY: AppKit's own constants, set before any code of the app's runs.
    let (light, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
    Some(Accents {
        light: under(light)?,
        dark: under(dark)?,
    })
}

/// The accent as the appearance named `name` draws it. It's a dynamic colour, which settles
/// on one shade only when converted, under the drawing appearance current then; converted
/// into sRGB, its components are ones the palette can take.
fn under(name: &NSAppearanceName) -> Option<Hsla> {
    let appearance = NSAppearance::appearanceNamed(name)?;
    let resolved = Cell::new(None);
    let resolve = StackBlock::new(|| {
        let srgb = NSColorSpace::sRGBColorSpace();
        let color = NSColor::controlAccentColor().colorUsingColorSpace(&srgb);
        resolved.set(color.map(|color| {
            accent::from_srgb(
                color.redComponent(),
                color.greenComponent(),
                color.blueComponent(),
                color.alphaComponent(),
            )
        }));
    });
    appearance.performAsCurrentDrawingAppearance(&resolve);
    resolved.get()
}

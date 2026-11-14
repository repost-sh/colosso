//! A floating window that is one pane of Liquid Glass, as macOS's app switcher is
//! (`docs/tab-switcher.md` 3): an `NSGlassEffectView` under the view GPUI draws in, where GPUI
//! puts its own blur for a blurred window, in a window that's see-through and has no edge of its
//! own. So macOS draws the glass's bright rim and the window's shadow round it, which glass
//! inside a window can't have (`liquid.rs`), and the glass shows whatever is behind the window.
//!
//! The glass is the app switcher's own, the owner's pick: a private subvariant of the view,
//! `appSwitcher`, set only where the view answers to it, as `liquid.rs` sets its private
//! `adaptiveAppearance`. Where it doesn't, the public Regular style, which is frosted too.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, AsyncApp, Window, WindowBackgroundAppearance};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol as _;
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSGlassEffectViewStyle, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::NSString;

use super::liquid;

/// `AppKit`'s `NSWindowBelow`: under the views already there.
const BELOW: isize = -1;

/// Makes `window`, a floating one being opened, a pane of the app switcher's glass, its corners
/// `radius` round, dark or light as the theme is. False, and the window is left as it opened,
/// where there's no such glass: before macOS 26, or in a test's window.
pub fn fill(window: &mut Window, radius: f32, cx: &mut App) -> bool {
    let (Some(main), Some(ns_window)) = (MainThreadMarker::new(), super::window::ns_window(window))
    else {
        return false;
    };
    let Some(content) = ns_window.contentView().filter(|_| liquid::has_glass()) else {
        return false;
    };
    let glass = liquid::new_glass(main);
    liquid::appear(&glass, cx.theme().mode.is_dark());
    if glass.respondsToSelector(sel!(set_subvariant:)) {
        let name = NSString::from_str("appSwitcher");
        // SAFETY: the private setter takes an `NSString`, the subvariant's name.
        let _: () = unsafe { msg_send![&glass, set_subvariant: &*name] };
    } else {
        glass.setStyle(NSGlassEffectViewStyle::Regular);
    }
    glass.setFrame(content.bounds());
    glass.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    glass.setCornerRadius(f64::from(radius));
    // SAFETY: `addSubview:positioned:relativeTo:` takes a view, an `NSWindowOrderingMode` and a
    // view to order against, none for all of them; `glass` is a view made on the main thread.
    unsafe {
        let _: () = msg_send![
            &content,
            addSubview: &*glass,
            positioned: BELOW,
            relativeTo: None::<&NSView>
        ];
    }
    window.set_background_appearance(WindowBackgroundAppearance::Transparent);
    borderless(ns_window, cx);
    true
}

/// Takes the title bar's style off `window`, which GPUI opens titled: macOS then draws no
/// corners, rim or shadow of its own round it, which would show round the glass's from outside
/// them, a second edge.
///
/// Done once the app is free of the update asking for it, as `window::fit` resizes: `AppKit`
/// makes the window's frame anew, telling GPUI of what changes with it as it happens, which
/// can't be heard mid-update.
fn borderless(window: Retained<NSWindow>, cx: &mut App) {
    cx.spawn(async move |_: &mut AsyncApp| {
        window.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
        window.setHasShadow(true);
    })
    .detach();
}

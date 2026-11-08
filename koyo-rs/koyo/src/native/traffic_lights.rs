//! The window's traffic lights, faded with the top rail they sit on: compact mode has none, as
//! Arc's hasn't. Gone, they're hidden as well, so a click where they were moves the window
//! rather than closing it. Shown again, `AppKit` lays them out where it would by default, high
//! in the corner, so they're put back level with the top rail's buttons. In full screen they're
//! left as they are, in the bar `AppKit` drops from the top of the screen.

use gpui_kit::{Window, point, px};
use objc2_app_kit::{NSWindowButton, NSWindowStyleMask};

use super::window::ns_window;
use crate::theme;

const BUTTONS: [NSWindowButton; 3] = [
    NSWindowButton::CloseButton,
    NSWindowButton::MiniaturizeButton,
    NSWindowButton::ZoomButton,
];

/// Shows `window`'s traffic lights `alpha` of the way, from gone (0) to as `AppKit` draws them
/// (1), touching only what differs.
pub fn fade(window: &Window, alpha: f32) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    let alpha = if ns_window
        .styleMask()
        .contains(NSWindowStyleMask::FullScreen)
    {
        1.
    } else {
        f64::from(alpha)
    };
    let hidden = alpha <= 0.;
    let mut changed = false;
    for button in BUTTONS
        .into_iter()
        .filter_map(|kind| ns_window.standardWindowButton(kind))
    {
        if (button.alphaValue() - alpha).abs() > f64::EPSILON {
            button.setAlphaValue(alpha);
            changed = true;
        }
        if button.isHidden() != hidden {
            button.setHidden(hidden);
            changed = true;
        }
    }
    if changed && !hidden {
        window.set_traffic_light_position(point(
            px(theme::TRAFFIC_LIGHTS_LEFT),
            px(theme::TRAFFIC_LIGHTS_TOP),
        ));
    }
}

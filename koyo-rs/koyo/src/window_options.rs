//! What every window Koyo opens starts from, so a choice made for all of them is made once.
//! A window builds its options as `WindowOptions { .., ..window_options::base() }`, never from
//! GPUI's own default, and a test holds every window to it.
//!
//! GPUI's default draws a window at most 30 times a second while macOS is in Low Power Mode
//! (gpui-fast's `WindowOptions::low_power_frame_interval`). macOS keeps the display's full rate
//! there, as fast as 120 times a second, and Arc draws at it: in Koyo a space's slide, a
//! page's scroll and a terminal's output went at a quarter of it. So a Koyo window keeps the
//! display's rate in Low Power Mode. GPUI's other two caps stay: a window behind another app's
//! draws at most 30 times a second, and one on a Mac under serious or critical thermal pressure
//! 60 times.

use gpui_kit::WindowOptions;

/// GPUI's default options, drawn at the display's full rate in Low Power Mode.
pub fn base() -> WindowOptions {
    WindowOptions {
        low_power_frame_interval: None,
        ..WindowOptions::default()
    }
}

#[cfg(test)]
mod tests;

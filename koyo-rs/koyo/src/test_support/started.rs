//! The app as a test or a visual check starts it: set up as the app sets itself up, with the
//! right to block lent where nothing draws.

use gpui_kit::App;

use crate::blocking::in_test;

/// What the app sets up before its first window, as a test or a visual check sets it up.
pub fn init(cx: &mut App) {
    in_test(|may| crate::init(cx, may));
}

/// The settings started as the app starts them; a test's keep no file.
#[cfg(test)]
pub fn init_settings(cx: &mut App) {
    in_test(|may| crate::settings::init(cx, may));
}

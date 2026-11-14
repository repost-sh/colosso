//! Where pages are never native: off macOS, and in tests, which start no engine in their own
//! process. The same few calls as `browser/native`, answering that pages are off screen.

use gpui_kit::App;
use koyo_browser_ipc::client::{Embedded, Launch};

pub fn decide(_: &App) {}

pub fn enabled() -> bool {
    false
}

pub fn embedded(_: &Launch) -> Option<Embedded> {
    None
}

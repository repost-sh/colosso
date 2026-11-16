//! Noticing what the backdrop shows changing: for now, the screens Koyo's windows are on, for the
//! memory the Quality row names, once a second.

use std::time::Duration;

use gpui_kit::App;

use super::screens;

/// How often the screens are looked at.
const EVERY: Duration = Duration::from_secs(1);

/// Starts noticing, from the main thread, which tests don't run on.
pub fn init(cx: &mut App) {
    if objc2::MainThreadMarker::new().is_none() {
        return;
    }
    screens::note_windows(cx);
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(EVERY).await;
            cx.update(screens::note_windows);
        }
    })
    .detach();
}

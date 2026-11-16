//! The harness's keyboard: where each window's keyboard is, where a named part of a window was
//! last drawn, a window made key, and bursts of keys or wheel turns.

use std::path::Path;
use std::time::Duration;

use gpui_kit::AsyncApp;
use objc2_app_kit::NSWindow;

use super::{key, scroll, window};

/// `keyboard`: each window the keyboard is kept for, its holder, its first responder and whether
/// it's key, and whether the app is in front (`docs/keyboard.md`).
pub(super) fn keyboard() -> String {
    crate::keyboard::describe()
}

/// `bounds <name>`: where the part of a window called `name` was last drawn (`crate::marks`), in
/// points from that window's top left.
pub(super) fn bounds(name: &str) -> String {
    let Some((window, bounds)) = crate::marks::find(name) else {
        return "not drawn".into();
    };
    let (origin, size) = (bounds.origin, bounds.size);
    format!(
        "{} {} {} {} in window {window}",
        origin.x.as_f32(),
        origin.y.as_f32(),
        size.width.as_f32(),
        size.height.as_f32()
    )
}

/// `key`, alone: the window made the key window now, the app left where it is, as a click on the
/// agent desktop may not make it.
pub(super) fn make_key(window: &NSWindow) -> String {
    window.makeKeyWindow();
    format!("key {}", window.isKeyWindow())
}

/// `burst <keys|wheels> <count> <gap ms> [x y]`: inputs one after another, each stamped as
/// it's handed to the window, the stamps in `<kind>.json`.
pub(super) async fn burst(rest: &str, folder: &Path, cx: &AsyncApp) -> String {
    let words: Vec<&str> = rest.split(' ').collect();
    let kind = words.first().copied().unwrap_or("keys");
    let number = |at: usize, or: f64| words.get(at).and_then(|w| w.parse::<f64>().ok()).unwrap_or(or);
    let (count, gap) = (number(1, 50.) as usize, number(2, 100.) as u64);
    let mut sent = Vec::with_capacity(count);
    for _ in 0..count {
        if let Some(window) = window() {
            sent.push(wall_ms());
            if kind == "wheels" {
                scroll(&window, -3);
            } else {
                key(&window, 0, "a", 0, false);
            }
        }
        cx.background_executor().timer(Duration::from_millis(gap)).await;
    }
    let stamps: Vec<String> = sent.iter().map(|at| format!("{at:.3}")).collect();
    let _ = std::fs::write(folder.join(format!("{kind}.json")), format!("[{}]", stamps.join(",")));
    format!("{} {kind} sent", sent.len())
}

/// The time by the wall clock, in milliseconds, as a page reads it too.
fn wall_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |since| since.as_secs_f64() * 1000.)
}


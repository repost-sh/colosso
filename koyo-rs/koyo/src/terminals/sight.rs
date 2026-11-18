//! A terminal out of sight: its view makes no frames of the output it takes in, and its activity
//! panel reads none, until it's back in sight.

use gpui_kit::App;

use super::Terminals;

impl Terminals {
    /// Tells each terminal tab's view, and its activity panel, whether it's in sight now, as
    /// `seen` says of its terminal, when that changed.
    pub(crate) fn tell_sight(&self, seen: impl Fn(u64) -> bool, cx: &mut App) {
        for (id, made) in &self.views {
            let now = seen(*id);
            if made.seen.replace(now) == now {
                continue;
            }
            made.view.update(cx, |view, _| view.set_seen(now));
            if let Some(panel) = self.panels.get(id) {
                panel.update(cx, |panel, cx| panel.set_seen(now, cx));
            }
        }
    }
}

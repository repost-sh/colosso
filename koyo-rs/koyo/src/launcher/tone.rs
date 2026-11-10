//! The launcher's glass over a page: dark over a light page and light over a dark one
//! (`theme::glass`), what's on it in the palette of that tone. Read as it opens, so its first
//! frame has it, and again every so often while it's open, as the page under it changes. Over
//! anything else, or mostly so, it's the theme's, as dialogs' glass is.

use std::time::Duration;

use gpui_kit::{App, Bounds, Context, Entity, Pixels, Point, WeakEntity, Window};

use super::view::Launcher;
use crate::browser::{BrowserView, glass_dark};

/// How often the page under an open launcher is read again.
const EVERY: Duration = Duration::from_millis(400);

/// The tone of the launcher's glass, and what it's read from.
pub struct Tone {
    /// The pages in the tab shown in the window behind, which it may be over.
    pages: Vec<WeakEntity<BrowserView>>,
    /// Where the window behind is on screen.
    behind: Point<Pixels>,
    /// Whether the glass is dark; none where it's the theme's.
    dark: Option<bool>,
}

impl Tone {
    /// The tone of a launcher over `pages`, in a window at `behind` on screen.
    pub fn new(pages: &[Entity<BrowserView>], behind: Point<Pixels>) -> Self {
        Tone {
            pages: pages.iter().map(Entity::downgrade).collect(),
            behind,
            dark: None,
        }
    }

    /// Whether the glass is dark; none where it's the theme's.
    pub fn dark(&self) -> Option<bool> {
        self.dark
    }

    /// Reads the pages under `bounds`, the launcher's window on screen; true if its tone changed.
    pub fn read(&mut self, bounds: Bounds<Pixels>, cx: &App) -> bool {
        let region = Bounds::new(bounds.origin - self.behind, bounds.size);
        let pages: Vec<_> = self.pages.iter().filter_map(WeakEntity::upgrade).collect();
        let dark = glass_dark(pages.iter().map(|page| page.read(cx)), region, self.dark);
        let changed = dark != self.dark;
        self.dark = dark;
        changed
    }

    /// Reads the pages under the launcher in `window` again every so often, for as long as it's
    /// open, and draws it again when its tone changes; over no page, there's nothing to read.
    pub fn follow(&self, window: &mut Window, cx: &mut Context<Launcher>) {
        if self.pages.is_empty() {
            return;
        }
        cx.spawn_in(window, async move |launcher, cx| {
            loop {
                cx.background_executor().timer(EVERY).await;
                let read = launcher.update_in(cx, |launcher, window, cx| {
                    if launcher.tone.read(window.bounds(), cx) {
                        cx.notify();
                    }
                });
                if read.is_err() {
                    return;
                }
            }
        })
        .detach();
    }
}

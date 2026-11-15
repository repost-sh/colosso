//! What a browser pane says of itself: its title and address, its number in the host, and
//! whether it's a page or another page's `DevTools`.

use gpui_kit::Entity;
use koyo_browser_ipc::PaneId;

use super::{BrowserView, Kind};
use crate::browser::toolbar;

impl BrowserView {
    /// What the strip calls the pane.
    pub fn title(&self) -> String {
        match &self.kind {
            Kind::Page { .. } => toolbar::tab_title(&self.page.title, &self.page.url),
            Kind::DevTools { .. } => "DevTools".into(),
        }
    }

    /// The address the page shows, or the one it's opening.
    pub fn address(&self) -> String {
        if self.page.url.is_empty() {
            self.target.clone()
        } else {
            self.page.url.clone()
        }
    }

    /// The page's id in the host, which the keyboard knows it by.
    pub fn page_id(&self) -> PaneId {
        self.pane
    }

    /// The `DevTools` of another pane's page.
    pub fn is_inspector(&self) -> bool {
        matches!(self.kind, Kind::DevTools { .. })
    }

    pub fn is_start_page(&self) -> bool {
        self.target.is_empty() && self.page.url.is_empty()
    }

    /// The page `DevTools` inspect.
    pub fn inspected(&self) -> Option<&Entity<BrowserView>> {
        match &self.kind {
            Kind::DevTools { inspected, .. } => Some(inspected),
            Kind::Page { .. } => None,
        }
    }
}

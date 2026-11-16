//! Who opened a page, which the profile it's of goes by (`browser/profiles.rs`), and whether
//! it's open in a profile its pages no longer use, so it closes and opens again in the right
//! one (`Browsers::profiles_changed`).

use gpui_kit::App;

use super::{BrowserView, Kind};
use crate::browser::profiles::By;

impl BrowserView {
    /// Who opened the page, the person unless it's an agent's (`made_by`).
    pub fn by(&self) -> By {
        match self.kind {
            Kind::Page { by, .. } => by,
            Kind::DevTools { .. } => By::Person,
        }
    }

    /// The page is `made`'s, as one an agent's tool opened, or such a page opened, is: it opens
    /// in the profile that says (`browser/profiles.rs`).
    pub fn made_by(&mut self, made: By) {
        if let Kind::Page { by, .. } = &mut self.kind {
            *by = made;
        }
    }

    /// Whether the page is open, in the host of `epoch`, in a profile its pages no longer use:
    /// its project was put on another, the profile deleted, or agents' pages kept apart or no
    /// longer. One not open opens in the right one; `DevTools` are their page's.
    pub(in crate::browser) fn profile_moved(&self, epoch: u64, cx: &App) -> bool {
        let Kind::Page {
            profile,
            project,
            by,
        } = &self.kind
        else {
            return false;
        };
        self.opened == Some(epoch) && *profile != crate::browser::profile(project, *by, cx)
    }
}

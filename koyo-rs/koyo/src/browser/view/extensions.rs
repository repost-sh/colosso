//! What extensions ask of a page's view (`docs/extensions.md` 3): one made for a tab an
//! extension or a page opened, which the host made already; the profile a page is of; and
//! telling the host which page the person is on.

use std::path::Path;

use gpui_kit::{Context, Entity, WeakEntity, Window};
use koyo_browser_ipc::{AppRequest, PaneId};

use super::{BrowserView, Kind};
use crate::browser::profiles::By;

impl BrowserView {
    /// A page that `opener`'s page or an extension opened at `url` from one `by` made in
    /// `project`: the host made it already, as its pane `pane`, in its opener's profile, which
    /// is the one `project`'s pages have when `by` opens them, so it's open from the start, and
    /// sized and shown as any.
    /// After the engine starts again it opens as any page does, under that number.
    pub fn adopted(
        (pane, opener): (PaneId, WeakEntity<BrowserView>),
        url: String,
        (project, by): (&Path, By),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kind = Kind::Page {
            profile: crate::browser::profile(project, by, cx).to_owned(),
            project: project.to_owned(),
            by,
        };
        let mut view = Self::with_kind(Some(pane), url, kind, window, cx);
        view.opener = Some(opener);
        view.opened = Some(view.browsers.read(cx).epoch());
        view.told = None;
        view.tell_sight(cx);
        view.session_opened(cx);
        view
    }

    /// The page's number in the host.
    pub fn host_pane(&self) -> PaneId {
        self.pane
    }

    /// The page that opened this one in a tab or a window of its own, while it's open.
    pub fn opener(&self) -> Option<Entity<BrowserView>> {
        self.opener.as_ref()?.upgrade()
    }

    /// The profile the page is of; none for the developer tools, which are of none.
    pub(in crate::browser) fn profile(&self) -> Option<&str> {
        match &self.kind {
            Kind::Page { profile, .. } => Some(profile),
            Kind::DevTools { .. } => None,
        }
    }

    /// Tells the host the person is on this page, which extensions get as the current tab
    /// (`docs/extensions.md` 3): it has the keyboard, or was shown last. The developer tools
    /// are no tab of theirs.
    pub(in crate::browser) fn tell_active(&mut self, cx: &mut Context<Self>) {
        if !self.is_inspector() {
            let pane = self.pane;
            self.send_to_page(AppRequest::Active { pane }, cx);
        }
    }
}

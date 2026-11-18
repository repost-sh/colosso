//! A browser pane's page: made, or the address it opens once its tab is shown, or the start
//! page; and whose it is, the person's or an agent's.

use std::sync::Arc;

use gpui_kit::{App, Entity, Image, SharedString};

use crate::browser::BrowserView;
use crate::browser::profiles::By;

/// What a browser pane shows.
#[derive(Clone, Debug)]
pub struct Browser {
    page: Page,
    /// Whose page it is: an agent's when an agent's tool opened it, or such a page did
    /// (`browser/profiles.rs` `By`). It's kept with the window, and its page made again so
    /// (`BrowserView::made_by`).
    pub by: By,
}

impl Browser {
    /// The start page, the person's.
    pub(super) fn start() -> Self {
        Browser {
            page: Page::Start,
            by: By::Person,
        }
    }

    /// Showing `view`, whoever's its page is.
    pub(super) fn made(view: Entity<BrowserView>, cx: &App) -> Self {
        Browser {
            by: view.read(cx).by(),
            page: Page::Made(Box::new(Made::of(view, cx))),
        }
    }

    /// Waiting to open `unloaded`'s address once its tab is shown, `by`'s.
    pub(crate) fn waiting(unloaded: Unloaded, by: By) -> Self {
        Browser {
            page: Page::Unloaded(Box::new(unloaded)),
            by,
        }
    }

    /// Its page, once it's made.
    pub fn view(&self) -> Option<&Entity<BrowserView>> {
        match &self.page {
            Page::Made(made) => Some(&made.view),
            Page::Start | Page::Unloaded(_) => None,
        }
    }

    /// The page the window's own controls act on: its own, or the one its `DevTools` inspect
    /// (`Shell::shown_page`).
    pub fn steered_page(&self) -> Option<&Entity<BrowserView>> {
        match &self.page {
            Page::Made(made) => Some(made.inspected.as_ref().unwrap_or(&made.view)),
            Page::Start | Page::Unloaded(_) => None,
        }
    }

    /// It shows `view` from now on, and no longer holds an address to open.
    pub fn show(&mut self, view: Entity<BrowserView>, cx: &App) {
        self.page = Page::Made(Box::new(Made::of(view, cx)));
    }

    /// Its address and icon follow its page's, and `title` its title (`BrowserEvent::Changed`):
    /// whether any of them changed.
    pub(super) fn follow(&mut self, title: &mut SharedString, cx: &App) -> bool {
        let Page::Made(made) = &mut self.page else {
            return false;
        };
        let now = Made::of(made.view.clone(), cx);
        let said = now.view.read(cx).title();
        if **made == now && **title == *said {
            return false;
        }
        (**made, *title) = (now, said.into());
        true
    }

    /// Its page is closed: it holds what it opens once its tab is shown again, as a pinned
    /// page does once it's closed (`docs/arc-sidebar.md` 11).
    pub(super) fn unload(&mut self, unloaded: Unloaded) {
        self.page = Page::Unloaded(Box::new(unloaded));
    }

    /// Lets go of its page, for a tab kept closed: once the tab is back and shown, it opens the
    /// address it showed, its first load a visit as a page opened anew is.
    pub(super) fn let_go(&mut self) {
        match &mut self.page {
            Page::Start => {}
            Page::Made(made) => {
                let unloaded = Unloaded {
                    address: made.address.clone(),
                    favicon: made.favicon.clone(),
                    restored: false,
                };
                self.page = Page::Unloaded(Box::new(unloaded));
            }
            Page::Unloaded(unloaded) => unloaded.restored = false,
        }
    }

    /// What it opens once its tab is shown, while it has no view.
    pub fn unloaded(&self) -> Option<&Unloaded> {
        match &self.page {
            Page::Unloaded(unloaded) => Some(unloaded),
            Page::Start | Page::Made(_) => None,
        }
    }

    /// The address it opens once its tab is shown, while it has no view.
    pub fn unloaded_address(&self) -> Option<&str> {
        Some(&self.unloaded()?.address)
    }

    /// The address it shows, or opens once its tab is shown; none for the start page.
    pub fn address(&self) -> Option<&str> {
        match &self.page {
            Page::Made(made) => Some(&made.address),
            Page::Unloaded(unloaded) => Some(&unloaded.address),
            Page::Start => None,
        }
    }

    /// Whether it shows another page's `DevTools`.
    pub fn is_inspector(&self) -> bool {
        matches!(&self.page, Page::Made(made) if made.inspected.is_some())
    }

    /// Its site's icon: its page's, or the one kept for its address while it has no view.
    pub fn favicon(&self) -> Option<Arc<Image>> {
        match &self.page {
            Page::Made(made) => made.favicon.clone(),
            Page::Unloaded(unloaded) => unloaded.favicon.clone(),
            Page::Start => None,
        }
    }
}

/// A page, made, and what its tab shows of it beside the title, as the view last said
/// (`Browser::follow`): its address (`BrowserView::address`) and its icon.
#[derive(Clone, Debug, PartialEq)]
struct Made {
    view: Entity<BrowserView>,
    /// The page the view's `DevTools` inspect, when it's theirs.
    inspected: Option<Entity<BrowserView>>,
    address: String,
    favicon: Option<Arc<Image>>,
}

impl Made {
    fn of(view: Entity<BrowserView>, cx: &App) -> Self {
        let page = view.read(cx);
        let (inspected, address, favicon) =
            (page.inspected().cloned(), page.address(), page.favicon());
        Made {
            view,
            inspected,
            address,
            favicon,
        }
    }
}

/// A page: the start page, with no address; made; or what it opens once its tab is shown.
#[derive(Clone, Debug)]
enum Page {
    Start,
    /// Boxed, as it keeps a pane small.
    Made(Box<Made>),
    /// A tab brought back at launch, its page made when the tab is first shown
    /// (`docs/browser.md` 5). Boxed, as few panes have one.
    Unloaded(Box<Unloaded>),
}

/// What a browser pane with no view yet opens once its tab is shown.
#[derive(Clone, Debug)]
pub struct Unloaded {
    pub address: String,
    /// The icon kept for the address's site, shown on the tab meanwhile.
    pub favicon: Option<Arc<Image>>,
    /// Whether it was visited before, as a page the window kept or a pinned page closed was,
    /// so its first load is no new visit (`BrowserView::mark_restored`). A closed tab's page
    /// brought back with ⌘⇧T isn't: its first load is one, as a page opened anew is.
    pub restored: bool,
}

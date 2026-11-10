//! Where a page goes: an address the person typed, a link followed, back, forward or again;
//! and how it came, which the history's visit of it says once it's loaded (`docs/new-tab.md`
//! 4.1). A page typed counts twice what one followed to does, and one gone back or forward to
//! isn't a new visit, as Firefox counts none for its session history; nor is the page shown
//! already, reloaded. What the person typed in the address that isn't one is a search, which
//! the history keeps too.

use gpui_kit::{App, Context};
use koyo_browser_ipc::{AppRequest, Go};

use super::{BrowserEvent, BrowserView, Notice};
use crate::browser::control::Mode;
use crate::browser::host::HostStatus;
use crate::browser::{Address, read_address, search_url};
use crate::history::How;

/// A page the pane loaded, as the history keeps it: `how` it came, or `None` when it was gone
/// back or forward to, or is the page the pane showed already, reloaded.
#[derive(Clone, Debug, PartialEq)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub how: Option<How>,
}

/// What the back, forward and reload buttons can do for the page: the toolbar's, and the Arc
/// sidebar's (`docs/arc-sidebar.md` 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a button each, enabled or not, and whether the page loads"
)]
pub struct Controls {
    pub back: bool,
    pub forward: bool,
    pub reload: bool,
    /// Reload stops the page instead while it loads.
    pub loading: bool,
}

impl BrowserView {
    /// What the buttons can do for the page now.
    pub fn controls(&self, cx: &App) -> Controls {
        let open = self.is_open(cx);
        let crashed = matches!(self.notice, Some(Notice::Gone(_)));
        Controls {
            back: open && self.page.can_go_back,
            forward: open && self.page.can_go_forward,
            reload: open || crashed,
            loading: self.page.loading,
        }
    }

    /// What the reload button does: stops the page while it loads, reloads it otherwise.
    pub fn reload_or_stop(&mut self, cx: &mut Context<Self>) {
        let to = if self.page.loading {
            Go::Stop
        } else {
            Go::Reload
        };
        self.go(to, cx);
    }

    /// Opens the page at `url`, as a link or an agent asks.
    pub fn navigate(&mut self, url: String, cx: &mut Context<Self>) {
        self.load(url, How::Followed, cx);
    }

    /// Opens the page at `url`, the person having asked for it by name.
    pub fn navigate_typed(&mut self, url: String, cx: &mut Context<Self>) {
        self.load(url, How::Typed, cx);
    }

    /// The page the pane opens first was asked for by name.
    pub fn mark_typed(&mut self) {
        self.came = Some(How::Typed);
    }

    /// The page the pane opens first is a tab's brought back at launch, visited before: its
    /// load is no new visit, as Firefox counts none for a restored session.
    pub fn mark_restored(&mut self) {
        self.came = None;
    }

    /// Goes where the person typed `text` in the address: to the page, or a search for it.
    pub(in crate::browser) fn go_to_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(address) = read_address(text) {
            self.go_to(address, cx);
        }
    }

    /// Goes to `address`, the person having asked for it by name, in the page's address or in
    /// the launcher opened on the page (⌘L): to the page, or a search for the words, which
    /// the history keeps. The page takes the keyboard after.
    pub(crate) fn go_to(&mut self, address: Address, cx: &mut Context<Self>) {
        self.person_acts(Mode::Implicit, cx);
        match address {
            Address::Page(url) => self.navigate_typed(url, cx),
            Address::Search(words) => {
                let url = search_url(&words);
                cx.emit(BrowserEvent::Searched(words));
                self.navigate(url, cx);
            }
        }
        self.settle.focus_page = true;
    }

    fn load(&mut self, url: String, came: How, cx: &mut Context<Self>) {
        if self.is_inspector() {
            return;
        }
        if matches!(self.status(cx), HostStatus::Failed(_)) {
            // Asking for a page again is the way to retry.
            self.restart_host(cx);
        }
        self.came = Some(came);
        self.notice = None;
        self.target.clone_from(&url);
        self.page.url.clone_from(&url);
        (self.page.loading, self.page.progress) = (true, 0);
        self.send_or_open(
            AppRequest::Navigate {
                pane: self.pane,
                url,
            },
            cx,
        );
        cx.emit(BrowserEvent::Changed);
        cx.notify();
    }

    /// The person moves through the page's history, reloads or stops it: from the page's toolbar,
    /// or the Arc sidebar's buttons.
    pub(crate) fn go(&mut self, to: Go, cx: &mut Context<Self>) {
        self.person_acts(Mode::Implicit, cx);
        if to == Go::Reload && matches!(self.notice, Some(Notice::Gone(_))) {
            // A crashed page needs a new browser, not a reload.
            self.notice = None;
            self.opened = None;
            self.ensure_open(cx);
            return;
        }
        if matches!(to, Go::Back | Go::Forward) {
            self.goes_through_history();
        }
        let pane = self.pane;
        self.send_to_page(AppRequest::Go { pane, to }, cx);
    }

    /// The page goes back or forward through its history, which is no new visit.
    pub(in crate::browser) fn goes_through_history(&mut self) {
        self.came = None;
    }

    /// The page the pane shows, loaded, and its title; `None` while it loads, or failed to.
    pub fn loaded_page(&self) -> Option<(&str, &str)> {
        let loaded = !self.page.loading && !self.page.url.is_empty() && self.notice.is_none();
        (loaded && !self.is_inspector())
            .then_some((self.page.url.as_str(), self.page.title.as_str()))
    }

    /// The page finished loading, or moved to another address loaded: a visit, how it came told
    /// once, as what follows came by a link unless the person says otherwise.
    pub(super) fn loaded(&mut self, cx: &mut Context<Self>) {
        let came = self.came.replace(How::Followed);
        let Some((url, title)) = self.loaded_page() else {
            return;
        };
        let again = self.visited.as_deref() == Some(url);
        let visit = Visit {
            url: url.to_owned(),
            title: title.to_owned(),
            how: came.filter(|_| !again),
        };
        self.visited = Some(visit.url.clone());
        cx.emit(BrowserEvent::Visited(visit));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    use gpui_kit::{AppContext as _, TestAppContext, px, size};
    use koyo_browser_ipc::PageState;

    use super::*;
    use crate::test_support::in_window;

    /// Each page told as visited, and how it came.
    type Told = Vec<(String, Option<How>)>;

    #[gpui_kit::test]
    fn a_page_is_told_once_loaded_how_it_came_and_a_reload_or_back_is_no_new_visit(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        let (_, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
            cx.new(|cx| BrowserView::new(String::new(), Path::new("/work/app"), window, cx))
        });
        let told: Rc<RefCell<Told>> = Rc::default();
        let heard = Rc::clone(&told);
        cx.update(|cx| {
            cx.subscribe(&view, move |_, event: &BrowserEvent, _| {
                if let BrowserEvent::Visited(visit) = event {
                    heard.borrow_mut().push((visit.url.clone(), visit.how));
                }
            })
            .detach();
        });
        let page = |url: &str, loading: bool, title: &str| PageState {
            url: url.into(),
            title: title.into(),
            loading,
            ..PageState::default()
        };
        let (a, b, c) = (
            "https://a.example/",
            "https://b.example/",
            "https://c.example/",
        );
        view.update(cx, |view, cx| {
            view.came = Some(How::Typed);
            view.page_changed(page(a, true, ""), cx);
            view.page_changed(page(a, false, "A"), cx);
            // Reloaded, it's the page shown already.
            view.page_changed(page(a, true, "A"), cx);
            view.page_changed(page(a, false, "A"), cx);
            // A link the page followed, then back: a title that comes meanwhile tells nothing.
            view.page_changed(page(b, false, "B"), cx);
            view.goes_through_history();
            view.page_changed(page(b, false, "B, retitled"), cx);
            view.page_changed(page(a, true, "A"), cx);
            view.page_changed(page(a, false, "A"), cx);
            view.page_changed(page(c, false, "C"), cx);
        });
        let typed = Some(How::Typed);
        let followed = Some(How::Followed);
        assert_eq!(
            *told.borrow(),
            [
                (a.into(), typed),
                (a.into(), None),
                (b.into(), followed),
                (a.into(), None),
                (c.into(), followed)
            ]
        );
    }
}

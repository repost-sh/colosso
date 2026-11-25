//! Browser tabs in the shell: opened with a page of the selected checkout's project, titled
//! as their page is, links from them opened beside them, and a page's detached `DevTools`
//! split into its tab, closing with it. A page's events go this one way whichever kind of tab
//! it's in, a checkout's, a pinned page's or a loose one's, each found where it is
//! (`Workspace::locate`), with only what's a pinned or a loose page's rule apart. Closing them
//! is `closing.rs`'s.

use std::path::{Path, PathBuf};

use gpui_kit::{AppContext as _, Context, Entity, WeakEntity, Window};

use super::Shell;
use crate::browser::profiles::By;
use crate::browser::{self, BrowserEvent, BrowserView, Responsive};
use crate::history::Place;
use crate::keyboard::Intent;
use crate::model::{Edge, Located, Loose, Pane, Spot, Tab};

impl Shell {
    /// Opens a browser tab on the selected checkout, at `url` or on the start page with
    /// the address ready to type in, and shows it.
    pub fn open_browser(
        &mut self,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_browser_tab(url, false, None, window, cx);
    }

    /// Opens a browser tab on the selected checkout, at `url` or on the start page with the
    /// address ready to type in, after its last tab or, when `beside`, right after the tab
    /// shown, its page starting in responsive `mode` when one is given (an agent's page,
    /// `docs/responsive.md` 9); shows it, and gives back its pane.
    pub(crate) fn open_browser_tab(
        &mut self,
        url: Option<String>,
        beside: bool,
        mode: Option<Responsive>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let loose = self.opens_loose();
        self.open_page(url, (loose, beside), mode.filter(|_| !loose), window, cx)
    }

    /// Opens a page at `url`, or the start page, and shows it: when `loose`, a loose page of the
    /// space shown (`docs/arc-sidebar.md` 11), after the loose page shown or after them all, of
    /// the person's own profile, taking the keyboard; else a tab of the selected checkout, as
    /// `open_browser_tab` says. Its pane.
    pub(super) fn open_page(
        &mut self,
        url: Option<String>,
        (loose, beside): (bool, bool),
        mode: Option<Responsive>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let project = if loose {
            PathBuf::from(browser::NO_PROJECT)
        } else if let Some(project) = self.workspace.project() {
            project.path.clone()
        } else {
            log::warn!("no checkout is selected to open a browser tab on");
            return None;
        };
        let url = url.unwrap_or_default();
        let start = url.is_empty();
        let view = self.browser_view(url, &project, window, cx);
        if let Some(mode) = mode {
            view.update(cx, |view, _| view.start_in(mode));
        }
        let pane = Pane::browser(view.clone(), cx);
        let (id, tab) = (pane.id, Tab::Single(pane));
        if loose {
            let place = (self.workspace.loose_shown()).map_or(usize::MAX, |shown| shown + 1);
            (self.workspace).add_loose(Loose { tab, project }, place, true);
        } else if beside {
            self.workspace.open_beside_shown(tab);
        } else {
            self.workspace.push_tab(tab);
        }
        let intent = if beside { Intent::LinkBeside } else { Intent::NewTab };
        if start {
            view.update(cx, |view, cx| view.focus_address(intent, window, cx));
        } else {
            self.intend(intent);
            self.keys_to_shown_tab(cx);
        }
        cx.notify();
        Some(id)
    }

    /// Opens a copy of the pinned or the loose page known by `key` as a tab of the selected
    /// checkout, as that page's menu does: at the address its page shows, or opens at, of the
    /// checkout's project's profile, after its tabs, and shown. Nothing with no checkout
    /// selected.
    pub(crate) fn copy_to_worktree(
        &mut self,
        key: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.checkout_shown().is_none() {
            return;
        }
        let Some(address) = self.workspace.pane(key).and_then(Pane::address) else {
            return;
        };
        self.open_page(Some(address.to_owned()), (false, false), None, window, cx);
    }

    /// A page of `project`'s profile at `url`, the shell hearing what it says.
    pub(crate) fn browser_view(
        &mut self,
        url: String,
        project: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        let view = cx.new(|cx| BrowserView::new(url, project, window, cx));
        cx.subscribe_in(&view, window, Self::browser_event).detach();
        view
    }

    pub(super) fn browser_event(
        &mut self,
        view: &Entity<BrowserView>,
        event: &BrowserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            BrowserEvent::Changed => {
                self.follow_page(view, cx);
                self.note_page(view, cx);
            }
            BrowserEvent::Visited(visit) => self.note_visit(view, visit, cx),
            BrowserEvent::Searched(words) => self.note_search(view, words, cx),
            BrowserEvent::SessionChanged => self.keep_window(cx),
            BrowserEvent::OpenTab(url) => drop(self.open_beside(view, None, url.clone(), window, cx)),
            BrowserEvent::GoToTab(url) => {
                let opened = self.open_beside(view, None, url.clone(), window, cx);
                if let Some(pane) = opened.and_then(|opened| self.pane_showing(&opened)) {
                    self.show_tab_holding(pane, cx);
                }
            }
            BrowserEvent::Adopted { pane, url, active } => {
                self.tab_adopted(view, *pane, url.clone(), *active, window, cx);
            }
            BrowserEvent::Activate => self.tab_asked_for(view, cx),
            BrowserEvent::AsksToLeave => self.page_asks_to_leave(view, window, cx),
            BrowserEvent::ClosedItself => self.page_closed_itself(view, cx),
            BrowserEvent::Detach(tools) => self.detach_devtools(view, tools, window, cx),
            BrowserEvent::Undock(tools) => self.remove_browser_pane(tools, cx),
            BrowserEvent::Closed if view.read(cx).is_inspector() => {
                self.remove_browser_pane(view, cx);
            }
            BrowserEvent::Closed => {}
        }
    }

    /// The pane showing `view`, wherever its tab is, a checkout's, a pinned page's or a loose
    /// one's: where that tab is, and the pane's id.
    pub(crate) fn locate_view(&self, view: &Entity<BrowserView>) -> Option<Located> {
        self.workspace.locate(|pane| pane.view() == Some(view))
    }

    /// The pane showing `view`, wherever it is: its id.
    pub(crate) fn pane_showing(&self, view: &Entity<BrowserView>) -> Option<u64> {
        self.locate_view(view).map(|located| located.pane)
    }

    /// The tab's title, address and icon follow its page's, a pinned or a loose page's too: the
    /// window draws again only when one of them changed.
    fn follow_page(&mut self, view: &Entity<BrowserView>, cx: &mut Context<Self>) {
        let showing = self.workspace.pane_mut_where(|pane| pane.view() == Some(view));
        if showing.is_some_and(|pane| pane.follow_page(cx)) {
            cx.notify();
        }
    }

    /// A page at `url` of the profile `project`'s pages have when `by` opens them, the shell
    /// hearing what it says: a new one, or with `adopted` the one the host made already under
    /// that number, in the profile of the opener given with it.
    pub(crate) fn page_view(
        &mut self,
        adopted: Option<(koyo_browser_ipc::PaneId, WeakEntity<BrowserView>)>,
        url: String,
        (project, by): (&Path, By),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        let Some(adopted) = adopted else {
            let view = self.browser_view(url, project, window, cx);
            view.update(cx, |view, _| view.made_by(by));
            return view;
        };
        let view = cx.new(|cx| BrowserView::adopted(adopted, url, (project, by), window, cx));
        cx.subscribe_in(&view, window, Self::browser_event).detach();
        view
    }

    /// `view`'s page opened a page in a tab or a window of its own, or an extension opened a
    /// tab from it, which the engine made as its pane `pane`: it gets a tab right after
    /// `view`'s, wherever that is, shown when `active`.
    pub(super) fn tab_adopted(
        &mut self,
        view: &Entity<BrowserView>,
        pane: koyo_browser_ipc::PaneId,
        url: String,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        drop(self.open_beside(view, Some(pane), url, window, cx));
        let opened = (self.workspace)
            .locate(|shown| shown.view().is_some_and(|page| page.read(cx).host_pane() == pane));
        match opened {
            Some(opened) if active => drop(self.show_tab_holding(opened.pane, cx)),
            Some(_) => {}
            None => browser::close_page(pane, cx),
        }
    }

    /// `view`'s page closed without the app asking: its tab goes, and when the person was
    /// looking at it, they're back on the page that opened it, if it did and is still open, as
    /// a sign-in's popup closing itself leaves them on the page they signed in to.
    fn page_closed_itself(&mut self, view: &Entity<BrowserView>, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_showing(view) else {
            return;
        };
        let shown = self.workspace.tab_shown().is_some_and(|tab| tab.has_pane(pane));
        let opener = (view.read(cx).opener()).and_then(|opener| self.pane_showing(&opener));
        self.close_pane_now(pane, cx);
        if let (true, Some(opener)) = (shown, opener) {
            self.show_tab_holding(opener, cx);
        }
    }

    /// `view`'s tab is wanted in front.
    pub(super) fn tab_asked_for(&mut self, view: &Entity<BrowserView>, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane_showing(view) {
            self.show_tab_holding(pane, cx);
        }
    }

    /// A link from `view`'s page, or from its `DevTools`, opens in a tab right after the tab
    /// it's in, without leaving it, of its opener's profile, so it's signed in as its opener
    /// is, and an agent's if its opener is: on its checkout, after its tab; from a pinned or a
    /// loose page, as a loose page of the space shown, right after a loose opener or after them
    /// all. With `adopted`, the page is one the host made already under that number, one a page
    /// or an extension opened (`BrowserView::adopted`). The page opened, when its opener's tab was
    /// found.
    pub(super) fn open_beside(
        &mut self,
        view: &Entity<BrowserView>,
        adopted: Option<koyo_browser_ipc::PaneId>,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<BrowserView>> {
        let located = self.locate_view(view)?;
        let Place { project, .. } = self.profile_place(located)?;
        let by = view.read(cx).inspected().unwrap_or(view).read(cx).by();
        let adopted = adopted.map(|pane| (pane, view.downgrade()));
        let opened = self.page_view(adopted, url, (&project, by), window, cx);
        let tab = Tab::Single(Pane::browser(opened.clone(), cx));
        match located.spot {
            Spot::Checkout(project_at, branch, at) => {
                self.workspace.insert_tab_after((project_at, branch, at), tab);
            }
            Spot::Pinned | Spot::Loose(_) => {
                let loose = self.workspace.loose_at(located.tab);
                let place = loose.map_or(usize::MAX, |beside| beside + 1);
                (self.workspace).add_loose(Loose { tab, project }, place, false);
            }
        }
        cx.notify();
        Some(opened)
    }

    /// Where the history keeps the visits of the page `located` finds, and the project whose
    /// profile it keeps: its checkout, or a pinned or a loose page's project.
    pub(super) fn profile_place(&self, located: Located) -> Option<Place> {
        match located.spot {
            Spot::Checkout(project, branch, _) => {
                let project = self.workspace.projects.get(project)?;
                let unit = project.branches.get(branch)?;
                Some(Place {
                    worktree: unit.worktree.path.clone(),
                    project: project.path.clone(),
                })
            }
            Spot::Pinned | Spot::Loose(_) => {
                let (_, project) = self.workspace.find_apart(|pane| pane.id == located.pane)?;
                Some(Place {
                    worktree: project.to_owned(),
                    project: project.to_owned(),
                })
            }
        }
    }

    /// `page`'s `DevTools` take a pane right of the page's, the shell hearing what they say.
    fn detach_devtools(
        &mut self,
        page: &Entity<BrowserView>,
        tools: &Entity<BrowserView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.pane_showing(page) else {
            return;
        };
        if self.pane_showing(tools).is_some() {
            return;
        }
        cx.subscribe_in(tools, window, Self::browser_event).detach();
        let beside = Tab::Single(Pane::browser(tools.clone(), cx));
        if let Some(tab) = self.workspace.tab_holding_mut(target) {
            tab.place(target, Edge::Right, beside);
            cx.notify();
        }
    }

    /// Takes the pane showing `view` out of its tab, wherever it is: a checkout's tab it was
    /// alone in closes, a pinned or a loose page's keeps its page (`Workspace::remove_pane`).
    pub(super) fn remove_browser_pane(
        &mut self,
        view: &Entity<BrowserView>,
        cx: &mut Context<Self>,
    ) {
        let removed = self.pane_showing(view).is_some_and(|id| self.workspace.remove_pane(id));
        if removed {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod opened;

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext};

    use gpui_kit::{Modifiers, VisualTestContext};

    use crate::menu::{self, MenuItem};
    use crate::model::{PaneKind, Tab};
    use crate::settings::Layout;
    use crate::shell::{Face, NewChat};
    use crate::test_support::{open_page_view, repo, select, shell_on, shell_with};

    #[gpui_kit::test]
    fn a_new_browser_tab_opens_on_the_selected_checkout(cx: &mut TestAppContext) {
        let root = repo("browser-tab", &[("README.md", "hi\n")]);
        let (window, shell) = shell_with(&[&root], cx);
        select(&shell, &root, cx);
        cx.update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| shell.open_browser(None, window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        shell.read_with(cx, |shell, cx| {
            let unit = shell.workspace.branch().expect("a checkout is selected");
            assert_eq!(unit.tabs.len(), 1, "one tab was opened");
            let Tab::Single(pane) = &unit.tabs[0] else {
                panic!("a single pane");
            };
            assert_eq!(pane.kind(), PaneKind::Browser);
            let view = pane.view().expect("the pane shows a browser view");
            assert!(view.read(cx).is_start_page());
        });
    }

    /// The page's tab shows its icon once it's fetched, and the address it's at once it moves.
    #[gpui_kit::test]
    fn a_pages_tab_shows_its_icon_and_address_as_they_change(cx: &mut TestAppContext) {
        let (window, shell, _) = shell_on("tab-follows-page", Layout::Orca, cx);
        let view = open_page_view(window, &shell, "https://example.com/", cx);
        let tab = |cx: &mut TestAppContext| {
            cx.read(|cx| {
                let pane = shell.read(cx).workspace.tab_shown().expect("the page's tab").first();
                (Face::of(pane, cx).favicon.is_some(), pane.address().map(str::to_owned))
            })
        };
        assert_eq!(tab(cx), (false, Some("https://example.com/".into())));

        let icon = "https://example.com/a.ico";
        view.update(cx, |view, cx| view.pretend_page("https://example.com/", Some(icon), cx));
        cx.run_until_parked();
        assert_eq!(tab(cx), (true, Some("https://example.com/".into())));

        view.update(cx, |view, cx| view.navigate("https://example.org/".into(), cx));
        cx.run_until_parked();
        assert_eq!(tab(cx), (true, Some("https://example.org/".into())));

        view.update(cx, |view, cx| view.pretend_page("https://example.org/", None, cx));
        cx.run_until_parked();
        assert_eq!(tab(cx), (false, Some("https://example.org/".into())));
    }

    /// A menu is a window of its own; picking a row there dispatches on the shell.
    #[gpui_kit::test]
    fn picking_new_chat_from_a_menu_window_opens_one(cx: &mut TestAppContext) {
        let root = repo("browser-tab-menu", &[("README.md", "hi\n")]);
        let (window, shell) = shell_with(&[&root], cx);
        select(&shell, &root, cx);
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear(cx);
            let focus = shell.read(cx).focus.clone();
            let items = vec![MenuItem::new(
                Some(gpui_kit::assets::IconName::Plus),
                "New Chat",
                Box::new(NewChat),
            )];
            menu::open_rows(
                crate::popup::Source::new("test-new-tab-menu"),
                items,
                None,
                &focus,
                window,
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
        let popup = cx
            .windows()
            .into_iter()
            .find(|handle| *handle != window)
            .expect("the menu opened a window of its own");
        let mut menu_cx = VisualTestContext::from_window(popup, cx);
        menu_cx.simulate_click(menu::row_center(0), Modifiers::default());
        cx.run_until_parked();
        shell.read_with(cx, |shell, _| {
            let unit = shell.workspace.branch().expect("a checkout is selected");
            assert_eq!(unit.tabs.len(), 1, "the row picked opened a tab");
            assert!(matches!(&unit.tabs[0], Tab::Single(pane) if pane.kind() == PaneKind::Chat));
        });
    }
}

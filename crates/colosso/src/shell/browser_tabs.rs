//! Browser tabs in the shell: opened with a page of the selected checkout's project, titled
//! as their page is, links from them opened beside them, and a page's detached `DevTools`
//! split into its tab, closing with it.

use gpui_kit::{AppContext as _, Context, Entity, Window};

use super::Shell;
use crate::browser::{BrowserEvent, BrowserView, Dock};
use crate::model::{ClosedTab, Edge, Pane, Tab};
use crate::navigation::Route;

impl Shell {
    /// Opens a browser tab on the selected checkout, at `url` or on the start page with
    /// the address ready to type in, and shows it.
    pub fn open_browser(
        &mut self,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.workspace.project().map(|project| project.path.clone()) else {
            log::warn!("no checkout is selected to open a browser tab on");
            return;
        };
        let url = url.unwrap_or_default();
        let start = url.is_empty();
        crate::browser::warm_up(cx);
        let view = self.browser_view(url, &project, window, cx);
        let pane = Pane::browser(view.clone(), cx);
        self.workspace.push_tab(Tab::Single(pane));
        if start {
            view.update(cx, |view, cx| view.focus_address(window, cx));
        }
        // A new tab is something to look at, so leave settings for it.
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    /// A page of `project`'s profile at `url`, the shell hearing what it says.
    pub(super) fn browser_view(
        &mut self,
        url: String,
        project: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        let view = cx.new(|cx| BrowserView::new(url, project, window, cx));
        cx.subscribe_in(&view, window, Self::browser_event).detach();
        view
    }

    fn browser_event(
        &mut self,
        view: &Entity<BrowserView>,
        event: &BrowserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            BrowserEvent::Changed => self.retitle(view, cx),
            BrowserEvent::OpenTab(url) => self.open_beside(view, url.clone(), window, cx),
            BrowserEvent::Detach(tools) => self.detach_devtools(view, tools, window, cx),
            BrowserEvent::Undock(tools) => self.remove_browser_pane(tools, cx),
            // DevTools the host closed with their page leave the tab.
            BrowserEvent::Closed if view.read(cx).is_inspector() => {
                self.remove_browser_pane(view, cx);
            }
            BrowserEvent::Closed => {}
        }
    }

    /// The pane showing `view`: where its tab is, as `(project, branch, tab)`, and its id.
    fn pane_of(&self, view: &Entity<BrowserView>) -> Option<((usize, usize, usize), u64)> {
        self.workspace
            .find_pane(|pane| pane.browser.as_ref() == Some(view))
    }

    /// Where the tab holding `view` is.
    fn tab_of(&self, view: &Entity<BrowserView>) -> Option<(usize, usize, usize)> {
        self.pane_of(view).map(|(at, _)| at)
    }

    /// The tab's title follows its page's.
    fn retitle(&mut self, view: &Entity<BrowserView>, cx: &mut Context<Self>) {
        let title = view.read(cx).title();
        let Some((at, id)) = self.pane_of(view) else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(at) else {
            return;
        };
        if let Some(pane) = tab.panes_mut().into_iter().find(|pane| pane.id == id) {
            pane.title = title.into();
            cx.notify();
        }
    }

    /// A link from `view`'s page opens in a tab right after it, without leaving it.
    fn open_beside(
        &mut self,
        view: &Entity<BrowserView>,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(at) = self.tab_of(view) else {
            return;
        };
        let project = self.workspace.projects[at.0].path.clone();
        let beside = self.browser_view(url, &project, window, cx);
        let pane = Pane::browser(beside, cx);
        self.workspace.insert_tab_after(at, Tab::Single(pane));
        cx.notify();
    }

    /// `page`'s `DevTools` take a pane right of the page's, the shell hearing what they say.
    fn detach_devtools(
        &mut self,
        page: &Entity<BrowserView>,
        tools: &Entity<BrowserView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((at, target)) = self.pane_of(page) else {
            return;
        };
        if self.tab_of(tools).is_some() {
            return;
        }
        cx.subscribe_in(tools, window, Self::browser_event).detach();
        let beside = Tab::Single(Pane::browser(tools.clone(), cx));
        if let Some(tab) = self.workspace.tab_mut(at) {
            tab.place(target, Edge::Right, beside);
            cx.notify();
        }
    }

    /// Takes the pane showing `view` out of its tab, closing the tab when it was alone.
    fn remove_browser_pane(&mut self, view: &Entity<BrowserView>, cx: &mut Context<Self>) {
        let Some(((project, branch, _), id)) = self.pane_of(view) else {
            return;
        };
        if let Some(unit) = self.workspace.unit_mut(project, branch) {
            unit.close_pane(id);
            cx.notify();
        }
    }

    /// Closes pane `id`, and its tab when it's the tab's only pane. Detached `DevTools`
    /// close for their page, and a page's detached `DevTools` close with it.
    pub(super) fn close_pane(&mut self, id: u64, cx: &mut Context<Self>) {
        let view = self
            .workspace
            .branch()
            .and_then(|unit| {
                unit.tabs
                    .iter()
                    .flat_map(Tab::panes)
                    .find(|pane| pane.id == id)
            })
            .and_then(|pane| pane.browser.clone());
        if let Some(view) = view {
            self.let_devtools_go(view, cx);
        }
        if let Some(unit) = self.workspace.branch_mut() {
            unit.close_pane(id);
        }
        cx.notify();
    }

    /// Closes the tab at `at`, as `(project, branch, tab)`, and the detached `DevTools` of
    /// its pages wherever they are, keeping it to open again.
    pub(super) fn close_tab_at(&mut self, at: (usize, usize, usize), cx: &mut Context<Self>) {
        let (project, branch, _) = at;
        let Some(tab) = self.workspace.tab_mut(at) else {
            return;
        };
        let panes: Vec<_> = tab
            .panes()
            .into_iter()
            .map(|pane| (pane.id, pane.browser.clone()))
            .collect();
        for view in panes.iter().filter_map(|(_, view)| view.clone()) {
            self.let_devtools_go(view, cx);
        }
        let addresses = (panes.iter())
            .filter_map(|(id, view)| {
                let view = view.as_ref()?.read(cx);
                (!view.is_inspector()).then(|| (*id, view.address()))
            })
            .collect();
        // Panes elsewhere that closed may have moved the tab.
        let unit = &self.workspace.projects[project].branches[branch];
        let checkout = unit.worktree.path.clone();
        if let Some(now) = panes.iter().find_map(|(id, _)| unit.tab_with(*id))
            && let Some(tab) = self.workspace.close_tab(project, branch, now)
        {
            self.workspace.remember_closed(ClosedTab {
                checkout,
                at: now,
                tab,
                addresses,
            });
        }
        cx.notify();
    }

    /// The `DevTools` of `view`'s page, or `view`'s own when it's a page, close; detached,
    /// their pane goes too.
    fn let_devtools_go(&mut self, view: Entity<BrowserView>, cx: &mut Context<Self>) {
        let page = view.read(cx).inspected().cloned().unwrap_or(view);
        let detached = page.read(cx).devtools_dock() == Some(Dock::Detached);
        let tools = page.read(cx).devtools().cloned().filter(|_| detached);
        page.update(cx, BrowserView::close_devtools);
        if let Some(tools) = tools {
            self.remove_browser_pane(&tools, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext};

    use gpui_kit::{Bounds, Modifiers, VisualTestContext};

    use crate::menu::{self, MenuItem};
    use crate::model::{PaneKind, Tab};
    use crate::shell::{NewBrowserTab, NewChat};
    use crate::test_support::{repo, select, shell_with};

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
            assert_eq!(pane.kind, PaneKind::Browser);
            let view = pane
                .browser
                .as_ref()
                .expect("the pane shows a browser view");
            assert!(view.read(cx).is_start_page());
        });
    }

    /// The "+" menu dispatches its action on the shell's focus handle.
    #[gpui_kit::test]
    fn the_new_tab_menus_action_opens_a_browser_tab(cx: &mut TestAppContext) {
        let root = repo("browser-tab-action", &[("README.md", "hi\n")]);
        let (window, shell) = shell_with(&[&root], cx);
        select(&shell, &root, cx);
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear(cx);
            let focus = shell.read(cx).focus.clone();
            focus.dispatch_action(&NewBrowserTab, window, cx);
        })
        .unwrap();
        cx.run_until_parked();
        shell.read_with(cx, |shell, _| {
            let unit = shell.workspace.branch().expect("a checkout is selected");
            assert_eq!(unit.tabs.len(), 1, "the action opened a tab");
            assert!(matches!(&unit.tabs[0], Tab::Single(pane) if pane.kind == PaneKind::Browser));
        });
    }

    /// The "+" menu is a window of its own; picking a row there dispatches on the shell.
    #[gpui_kit::test]
    fn picking_new_browser_tab_from_the_menu_window_opens_one(cx: &mut TestAppContext) {
        let root = repo("browser-tab-menu", &[("README.md", "hi\n")]);
        let (window, shell) = shell_with(&[&root], cx);
        select(&shell, &root, cx);
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear(cx);
            let focus = shell.read(cx).focus.clone();
            let items = vec![
                MenuItem {
                    icon: gpui_kit::assets::IconName::Plus,
                    label: "New Chat".into(),
                    action: Box::new(NewChat),
                },
                MenuItem {
                    icon: gpui_kit::assets::IconName::Globe,
                    label: "New Browser Tab".into(),
                    action: Box::new(NewBrowserTab),
                },
            ];
            menu::open(
                "test-new-tab-menu".into(),
                Bounds::default(),
                items,
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
        menu_cx.simulate_click(menu::row_center(1), Modifiers::default());
        cx.run_until_parked();
        shell.read_with(cx, |shell, _| {
            let unit = shell.workspace.branch().expect("a checkout is selected");
            assert_eq!(unit.tabs.len(), 1, "the row picked opened a tab");
            assert!(matches!(&unit.tabs[0], Tab::Single(pane) if pane.kind == PaneKind::Browser));
        });
    }
}

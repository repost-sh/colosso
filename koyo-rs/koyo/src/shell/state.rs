//! The window's switches as the shell's keys and buttons set them, and as app control reads
//! and sets them: the sidebar, the bottom panel, the settings pages, and the pane the person
//! is in, which has the keyboard or had it last.

use gpui_kit::{App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, Window};

use super::Shell;
use crate::files::FilePane;
use crate::model::{Pane, PaneKind, Tab, Workspace};
use crate::navigation::Route;
use crate::settings_view::SettingsPage;

/// What takes the keyboard for a pane: the focus of its chat, its terminal or its page, or its
/// file, which has its editor take it.
enum Keys {
    View(FocusHandle),
    File(Entity<FilePane>),
}

impl Keys {
    fn take(&self, window: &mut Window, cx: &mut App) {
        match self {
            Keys::View(focus) => window.focus(focus, cx),
            Keys::File(file) => file.update(cx, |file, cx| file.focus(window, cx)),
        }
    }

    /// Whether the keyboard is there now.
    fn held(&self, window: &Window, cx: &App) -> bool {
        match self {
            Keys::View(focus) => focus.contains_focused(window, cx),
            Keys::File(file) => file.read(cx).has_focus(window, cx),
        }
    }
}

/// Pane `id` of the tab at `at`, as `(project, branch, tab)`.
fn pane_in(workspace: &Workspace, at: (usize, usize, usize), id: u64) -> Option<&Pane> {
    let panes = workspace.tab(at)?.panes();
    panes.into_iter().find(|pane| pane.id == id)
}

impl Shell {
    /// Whether the sidebar is open, or sliding open; closed, the window is in compact mode.
    pub(crate) fn sidebar_open(&self) -> bool {
        self.sidebar_open
    }

    /// Opens or closes the sidebar, as ⌘B does, the rails sliding out or folding away with it.
    pub(crate) fn set_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.sidebar_open != open {
            self.sidebar_open = open;
            cx.notify();
        }
    }

    /// Whether the bottom panel is open.
    pub(crate) fn bottom_panel_open(&self) -> bool {
        self.bottom_panel_open
    }

    /// Opens or closes the bottom panel, as ⌘J does.
    pub(crate) fn set_bottom_panel_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.bottom_panel_open != open {
            self.bottom_panel_open = open;
            cx.notify();
        }
    }

    /// The window shows something else: Settings → Devices lists Koyo's simulators afresh each
    /// time it shows, and the window is drawn again.
    pub(super) fn went(&mut self, cx: &mut Context<Self>) {
        if self.shows(SettingsPage::Devices, cx) {
            crate::devices::devices(cx).update(cx, crate::devices::Devices::refresh);
        }
        cx.notify();
    }

    /// Whether Settings shows `page`.
    pub(super) fn shows(&self, page: SettingsPage, cx: &App) -> bool {
        self.route(cx) == Route::Settings(page)
    }

    /// Shows Settings on `page`, or on the page shown last when `None`, as ⌘, does.
    pub(crate) fn open_settings_page(
        &mut self,
        page: Option<SettingsPage>,
        cx: &mut Context<Self>,
    ) {
        let page = page.unwrap_or_else(|| self.history.read(cx).last_settings_page());
        self.navigate(Route::Settings(page), cx);
    }

    /// Shows the tab holding pane `id`, on whichever checkout it is, and gives the pane the
    /// keyboard; false when no such pane is open.
    pub(crate) fn focus_pane(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((at, _)) = self.workspace.find_pane(|pane| pane.id == id) else {
            return false;
        };
        self.workspace.select_tab(at.0, at.1, at.2);
        self.workspace.enter_pane(id);
        self.navigate(Route::Workspace, cx);
        self.follow_selection(window, cx);
        // What it shows is made as its tab is shown, to take the keyboard: a page brought back
        // at launch, a chat's view, a terminal's.
        self.make_shown_browsers(window, cx);
        self.make_shown_chats(window, cx);
        let terminal = pane_in(&self.workspace, at, id).and_then(|pane| pane.terminal);
        if let Some(terminal) = terminal {
            self.terminal_view(terminal, window, cx);
        }
        let keys = pane_in(&self.workspace, at, id).and_then(|pane| self.keys_of(pane, cx));
        match keys {
            Some(keys) => keys.take(window, cx),
            // A terminal whose view can't be made leaves the keyboard where it is.
            None if terminal.is_some() => {}
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
        true
    }

    /// What takes the keyboard for `pane`: its chat's, its terminal's or its page's view, once
    /// that's made, the agent desktop's, or its file.
    fn keys_of(&self, pane: &Pane, cx: &App) -> Option<Keys> {
        let chat = (self.chat_tabs.view(pane.id)).map(|view| view.focus_handle(cx));
        let terminal = || Some(self.terminals.view(pane.terminal?)?.focus_handle(cx));
        let browser = || pane.view().map(|view| view.focus_handle(cx));
        let desktop = || Some(pane.desktop_view()?.read(cx).focus_handle().clone());
        let device = || Some(pane.device_view()?.read(cx).focus_handle().clone());
        let view = (chat.or_else(terminal).or_else(browser))
            .or_else(desktop)
            .or_else(device);
        (view.map(Keys::View)).or_else(|| pane.file.clone().map(Keys::File))
    }

    /// The pane of the shown tab the person is in: the one with the keyboard, or, while the
    /// sidebar, a dialog or another window has it, the one they were in last (`Tab::current`),
    /// the tab's first until they've been in any.
    pub(crate) fn focused_pane(&self, window: &Window, cx: &App) -> Option<u64> {
        let tab = self.workspace.tab_shown()?;
        (self.pane_with_keyboard(window, cx)).or_else(|| Some(tab.current().id))
    }

    /// The pane of the shown tab that has the keyboard now, if one does.
    fn pane_with_keyboard(&self, window: &Window, cx: &App) -> Option<u64> {
        let panes = self.workspace.tab_shown()?.panes();
        let held =
            |pane: &&Pane| (self.keys_of(pane, cx)).is_some_and(|keys| keys.held(window, cx));
        panes.into_iter().find(held).map(|pane| pane.id)
    }

    /// Shown with nothing else holding the keyboard, a pane of the tab takes it, so its keys
    /// work as soon as its tab is picked in the sidebar: the pane the person was in, a file
    /// among them, else the tab's first terminal, chat or page. Only then, as reading a view
    /// while the window draws makes it draw again whenever that view changes.
    pub(super) fn give_shown_tab_the_keyboard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus.is_focused(window) {
            return;
        }
        let Some(tab) = self.workspace.tab_shown() else {
            return;
        };
        // Back in the pane they were in, rather than in whatever shares its tab. A file
        // they've only looked at leaves the keyboard where it is, as the tree previews one.
        let theirs = Some(tab.current()).filter(|pane| pane.file.is_none() || pane.entered_at > 0);
        let panes = tab.panes();
        let of = |kind: PaneKind| panes.iter().copied().filter(move |pane| pane.kind == kind);
        let Some((id, keys)) = (theirs.into_iter())
            .chain(of(PaneKind::Terminal))
            .chain(of(PaneKind::Chat))
            .chain(of(PaneKind::Browser))
            .chain(of(PaneKind::Device))
            .find_map(|pane| Some((pane.id, self.keys_of(pane, cx)?)))
        else {
            return;
        };
        cx.defer_in(window, move |this, window, cx| {
            keys.take(window, cx);
            this.enter_pane(id, cx);
        });
    }

    /// The person pressed in pane `id`, or gave it the keyboard: they're in it from now on, and
    /// what follows the pane they're in, as the Arc sidebar's address does, is drawn again.
    pub(crate) fn enter_pane(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.workspace.enter_pane(id) {
            cx.notify();
        }
    }

    /// The agent desktop's pane: the one that's open, wherever it is, or a new one in a
    /// tab of its own on the selected checkout. Its tab is shown when `shown`; else a new
    /// one comes after the tab that is, out of sight, as a tab an agent opens does. `None`
    /// with no checkout to open it on.
    pub fn desktop_pane(&mut self, shown: bool, cx: &mut Context<Self>) -> Option<u64> {
        let open = (self.workspace).find_pane(|pane| pane.kind == PaneKind::Desktop);
        if let Some(((project, branch, tab), pane)) = open {
            if shown {
                self.workspace.select_tab(project, branch, tab);
            }
            return Some(pane);
        }
        let pane = Pane::desktop(cx.new(crate::agent_desktop::DeskView::new));
        let id = pane.id;
        // Out of sight it comes after the tab that shows; with no tab showing there's
        // nothing to keep the person on, and it's the one shown.
        let beside = (self.workspace.shown_tab()).filter(|at| self.workspace.tab(*at).is_some());
        match beside {
            Some(beside) if !shown => (self.workspace).insert_tab_after(beside, Tab::Single(pane)),
            _ => self.workspace.push_tab(Tab::Single(pane)),
        }
        cx.notify();
        (self.workspace.find_pane(|pane| pane.id == id)).map(|_| id)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, Focusable as _, TestAppContext};

    use crate::model::PaneKind;
    use crate::test_support::{draw, open_page, repo, select, shell_with};

    /// Its keys, ⌘R, ⌘F and the rest, are bound where the page has the keyboard.
    #[gpui_kit::test]
    fn a_page_takes_the_keyboard_when_its_tab_is_picked_in_the_sidebar(cx: &mut TestAppContext) {
        let root = repo("shell-page-keyboard", &[("README.md", "hi\n")]);
        let (window, shell) = shell_with(&[&root], cx);
        select(&shell, &root, cx);
        let pane = open_page(window, &shell, Some("https://example.com/"), cx);
        let page_has_keyboard = |cx: &mut TestAppContext| {
            // Drawn, the keyboard's loss is heard; drawn again, the tab shown takes it.
            for _ in 0..3 {
                draw(window, cx);
            }
            cx.update_window(window, |_, window, cx| {
                let shell = shell.read(cx);
                let view = shell.workspace.branch()?.pane(pane)?.view()?.clone();
                let context = window.context_stack();
                let bound = context
                    .iter()
                    .any(|context| context.contains("BrowserPage"));
                Some(view.focus_handle(cx).is_focused(window) && bound)
            })
            .unwrap()
        };
        assert_eq!(page_has_keyboard(cx), Some(true), "a new tab at an address");

        // A chat takes the keyboard; its tab left for the page's, as a click on its row leaves
        // it, the page has it again.
        shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
        assert_eq!(page_has_keyboard(cx), Some(false));
        shell.update(cx, |shell, cx| {
            shell.workspace.select_tab(0, 0, 0);
            cx.notify();
        });
        assert_eq!(page_has_keyboard(cx), Some(true));
        let focused = cx.update_window(window, |_, window, cx| {
            shell.read(cx).focused_pane(window, cx)
        });
        assert_eq!(focused.unwrap(), Some(pane));
    }

    #[gpui_kit::test]
    fn the_keyboard_comes_back_to_the_file_the_person_was_in_not_to_the_chat_beside_it(
        cx: &mut TestAppContext,
    ) {
        let root = repo("shell-file-keyboard", &[("README.md", "hi\n")]);
        let (window, shell) = shell_with(&[&root], cx);
        select(&shell, &root, cx);
        // A chat on the left, the file on its right; and a tab after them.
        cx.update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.new_tab(PaneKind::Chat, cx);
                shell.open_file(
                    crate::files::Place::file("README.md"),
                    true,
                    false,
                    window,
                    cx,
                );
                let unit = shell.workspace.branch_mut().expect("a checkout");
                let chat = unit.tabs[0].first().id;
                assert!(unit.drop_tab(1, chat, crate::model::Edge::Right));
                cx.notify();
            });
        })
        .unwrap();
        let (chat, file) = shell.read_with(cx, |shell, _| {
            let panes = shell.workspace.tab_shown().expect("the split").panes();
            (panes[0].id, panes[1].id)
        });
        let settled = |cx: &mut TestAppContext| {
            for _ in 0..3 {
                draw(window, cx);
            }
            let focused = cx.update_window(window, |_, window, cx| {
                shell.read(cx).focused_pane(window, cx)
            });
            focused.unwrap()
        };
        // Leaving the tab and coming back, as a click on its row does, with the keyboard at
        // the window: a file only looked at leaves it to the chat.
        let away_and_back = |cx: &mut TestAppContext| {
            shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
            draw(window, cx);
            cx.update_window(window, |_, window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.workspace.select_tab(0, 0, 0);
                    let root = shell.focus.clone();
                    window.focus(&root, cx);
                    cx.notify();
                });
            })
            .unwrap();
        };
        away_and_back(cx);
        assert_eq!(settled(cx), Some(chat));

        // In the file, as a press in it puts them: it has the keyboard again on the way back.
        shell.update(cx, |shell, cx| shell.enter_pane(file, cx));
        away_and_back(cx);
        assert_eq!(settled(cx), Some(file), "the file they were in");
        let typing = cx.update_window(window, |_, window, cx| {
            let shell = shell.read(cx);
            let pane = shell.workspace.branch().and_then(|unit| unit.pane(file));
            pane.and_then(|pane| pane.file.clone())
                .is_some_and(|file| file.read(cx).has_focus(window, cx))
        });
        assert!(typing.unwrap(), "its editor has the keyboard");
    }
}

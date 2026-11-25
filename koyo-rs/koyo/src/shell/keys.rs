//! Where the keyboard is among the panes, which only the person moves (`docs/sidebar.md`
//! 9), and the pane they're in, which has the keyboard or had it last.
//!
//! It goes to a pane when they press in it, as its view takes it then, or when they ask for
//! its tab: picked, opened, switched to, or come back to (`Shell::keys_to_shown_tab`). Not to
//! a chat's prompt, though: moving to a chat, however it's done, leaves the keyboard at the
//! window, and the prompt has it once they press in it, or in a chat they've just opened to
//! type in (`Shell::keys_to_new_tab`). It leaves a pane when they press anything that takes
//! no keyboard, as the window's root takes it then (the chat's background, the sidebar, the
//! glass; a button leaves it where it was), and when what had it stops being shown, as
//! another space, project or worktree shown instead does (`on_focus_lost` in `Shell::new`).
//! At the window, the window's own keys work and nothing is typed into.
//!
//! What the person asks for is told to the keyboard's holder as it's asked, with what they
//! asked (`docs/keyboard.md`): the holder moves `AppKit`'s first responder into a page or out
//! of it to match. A tab shown again gives the keyboard back to the page that had it as it was
//! left, the page's own or its `DevTools`, which GPUI's focus alone can't tell apart.

use gpui_kit::{App, Context, Entity, FocusHandle, Focusable as _, Window};

use super::Shell;
use crate::files::FilePane;
use crate::browser::BrowserView;
use crate::keyboard::{self, Cause, Intent, Part, Surface};
use crate::model::{Content, Editor, EditorFile, Pane, PaneKind, Spot};

/// What's still to happen to the keyboard, between what the person did and the frame that
/// settles it.
#[derive(Default)]
pub(crate) struct Handing {
    /// The tab, by its first pane, that takes the keyboard as it's next drawn
    /// (`Shell::keys_to_shown_tab`), and whether a chat's prompt may: one just opened only.
    to: Option<(u64, bool)>,
    /// While the Settings tab is shown, whether a pane had the keyboard as it opened.
    left: Option<bool>,
    /// What the person asked for that hands it over, if they asked: else it's the tab shown.
    intent: Option<Intent>,
}

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

impl Shell {
    /// Shows the tab holding pane `id`, wherever it is, a checkout's, a pinned page's or a
    /// loose one's, and gives the pane the keyboard, a chat's excepted; false when no such
    /// pane is open.
    pub(crate) fn focus_pane(
        &mut self,
        id: u64,
        intent: Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.select_tab_holding(id, cx) {
            return false;
        }
        self.workspace.enter_pane(id);
        self.follow_selection(window, cx);
        self.make_shown(window, cx);
        let terminal = self.workspace.pane(id).and_then(Pane::terminal);
        let keys = (self.workspace.pane(id))
            .filter(|pane| pane.kind() != PaneKind::Chat)
            .and_then(|pane| Some((self.keys_of(pane, cx)?, surface_of(pane, cx))));
        let asked = Cause::Intent(intent);
        match keys {
            Some((keys, surface)) => {
                keyboard::set(window, surface, asked);
                keys.take(window, cx);
            }
            None if terminal.is_some() => {}
            None => {
                keyboard::set(window, Surface::Gpui(Part::Window), asked);
                window.focus(&self.focus, cx);
            }
        }
        cx.notify();
        true
    }

    /// Shows the tab holding pane `id` and gives it the keyboard, as a press on its row or its
    /// tile does, wherever it is: a checkout's tab in its checkout, a pinned page over the
    /// space shown, a loose page in its space. The one way a tab is shown when asked for, by
    /// the sidebar, ⌃⇥ or the launcher, so a page goes the same way whichever kind of tab it's
    /// in. False when no such pane is open.
    pub(crate) fn show_tab_holding(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let shown = self.select_tab_holding(id, cx);
        if shown {
            self.keys_to_shown_tab(cx);
        } else {
            self.keys.intent = None;
        }
        shown
    }

    /// Makes the tab holding pane `id` the one shown, wherever it is, a loose page's space
    /// coming to show with it unless the space shown lists it. Whether there's such a tab.
    fn select_tab_holding(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let Some(located) = self.workspace.locate(|pane| pane.id == id) else {
            return false;
        };
        if let Spot::Loose(space) = located.spot
            && !self.workspace.lists_loose_of(space)
        {
            self.show_space_known_by(space, cx);
        }
        let shown = self.workspace.show_tab(located.spot, located.tab);
        if shown {
            cx.notify();
        }
        shown
    }

    /// What takes the keyboard for `pane`: its chat's, its terminal's or its page's view, once
    /// that's made, the agent desktop's or a device's, or its file once it's open.
    fn keys_of(&self, pane: &Pane, cx: &App) -> Option<Keys> {
        let view = match &pane.content {
            Content::Chat(_) => self.chat_tabs.view(pane.id)?.focus_handle(cx),
            Content::Terminal(id) => self.terminals.view((*id)?)?.focus_handle(cx),
            Content::Browser(browser) => browser.view()?.focus_handle(cx),
            Content::Desktop(view) => view.as_ref()?.read(cx).focus_handle().clone(),
            Content::Device(device) => device.view()?.read(cx).focus_handle().clone(),
            Content::Editor(Editor::File(EditorFile::Open(file))) => {
                return Some(Keys::File(file.clone()));
            }
            Content::Editor(
                Editor::Untitled | Editor::File(EditorFile::Unopened(_)) | Editor::Changes(_),
            ) => return None,
        };
        Some(Keys::View(view))
    }

    /// The pane of the shown tab the person is in: the one with the keyboard, or, while the
    /// sidebar, a dialog or another window has it, the one they were in last (`Tab::current`),
    /// the tab's first until they've been in any.
    pub(crate) fn focused_pane(&self, window: &Window, cx: &App) -> Option<u64> {
        let tab = self.workspace.tab_shown()?;
        (self.pane_with_keyboard(window, cx)).or_else(|| Some(tab.current().id))
    }

    /// The pane of the shown tab that has the keyboard now, if one does.
    pub(super) fn pane_with_keyboard(&self, window: &Window, cx: &App) -> Option<u64> {
        let panes = self.workspace.tab_shown()?.panes();
        panes.into_iter().find(|pane| self.has_keyboard(pane, window, cx)).map(|pane| pane.id)
    }

    /// Whether `pane` has the keyboard now.
    fn has_keyboard(&self, pane: &Pane, window: &Window, cx: &App) -> bool {
        (self.keys_of(pane, cx)).is_some_and(|keys| keys.held(window, cx))
    }

    /// Shows the tab at `tab` of the checkout at `branch` of `project`, as a press on its row
    /// does: one of its panes takes the keyboard.
    pub(crate) fn pick_tab(
        &mut self,
        project: usize,
        branch: usize,
        tab: usize,
        cx: &mut Context<Self>,
    ) {
        let key = self.workspace.tab((project, branch, tab)).map(|tab| tab.first().id);
        if let Some(key) = key {
            self.show_tab_holding(key, cx);
        }
    }

    /// The person pressed in pane `id`, or gave it the keyboard: they're in it from now on, and
    /// what follows the pane they're in, as the sidebar's address does, is drawn again.
    pub(crate) fn enter_pane(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.workspace.enter_pane(id) {
            cx.notify();
        }
    }

    /// The tab shown takes the keyboard, as the person asked by picking it, switching to it
    /// or coming back to it: as its panes have their views, at the window's next frame, unless
    /// another tab shows first or it has nothing that takes a keyboard. A chat takes none: its
    /// prompt has the keyboard once they press in it.
    pub(crate) fn keys_to_shown_tab(&mut self, cx: &mut Context<Self>) {
        self.keys.to = self.shown_tab_key().map(|tab| (tab, false));
        if self.keys.to.is_none() {
            self.keys.intent = None;
        }
        cx.notify();
    }

    /// What the person asked for hands the keyboard to the tab shown next
    /// (`keys_to_shown_tab`), and the keyboard's holder hears it as `intent`.
    pub(crate) fn intend(&mut self, intent: Intent) {
        self.keys.intent = Some(intent);
    }

    /// The tab of `kind` the person just opened takes the keyboard, a chat's prompt too, as
    /// it's opened to type in; a terminal does once it's open (`open_terminal`).
    pub(super) fn keys_to_new_tab(&mut self, kind: PaneKind, cx: &mut Context<Self>) {
        if kind != PaneKind::Terminal {
            self.intend(Intent::NewTab);
            self.keys.to = (self.shown_tab_key()).map(|tab| (tab, kind == PaneKind::Chat));
            cx.notify();
        }
    }

    /// The tab shown, by its first pane.
    pub(crate) fn shown_tab_key(&self) -> Option<u64> {
        Some(self.workspace.tab_shown()?.first().id)
    }

    /// The tab asked to take the keyboard (`keys_to_shown_tab`) takes it, as the workspace
    /// draws it: the keyboard's holder is told so, with what the person asked if they did
    /// (`intend`), and GPUI's focus follows. It goes to the page the tab had it in as it was
    /// left, its own or its `DevTools`; else to the pane the person was in, a file among them,
    /// else to the tab's first terminal, chat, page or device. A file they've only looked at
    /// leaves the keyboard where it is, as the tree previews one, and so does a chat that
    /// wasn't just opened.
    pub(super) fn tab_takes_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((to, prompt)) = self.keys.to else {
            self.keys.intent = None;
            return;
        };
        self.keys.to = None;
        let cause = self.keys.intent.take().map_or(Cause::TabShown, Cause::Intent);
        let Some(tab) = (self.workspace.tab_shown()).filter(|tab| tab.first().id == to) else {
            return;
        };
        if let Some(Surface::Page(page)) = keyboard::tab_shown(window, to)
            && let Some((id, view)) = page_in(&tab.panes(), page, cx)
        {
            let focus = view.focus_handle(cx);
            cx.defer_in(window, move |this, window, cx| {
                keyboard::set(window, Surface::Page(page), cause);
                window.focus(&focus, cx);
                this.enter_pane(id, cx);
            });
            return;
        }
        let theirs =
            Some(tab.current()).filter(|pane| pane.file().is_none() || pane.entered_at > 0);
        let mut panes: Vec<_> = (tab.panes().into_iter())
            .filter_map(|pane| Some((takes_keys_after(pane.kind())?, pane)))
            .collect();
        panes.sort_by_key(|(after, _)| *after);
        let Some((pane, keys)) = (theirs.into_iter())
            .chain(panes.into_iter().map(|(_, pane)| pane))
            .find_map(|pane| Some((pane, self.keys_of(pane, cx)?)))
        else {
            return;
        };
        if pane.kind() == PaneKind::Chat && !prompt {
            return;
        }
        let (id, surface) = (pane.id, surface_of(pane, cx));
        cx.defer_in(window, move |this, window, cx| {
            keyboard::set(window, surface, cause);
            keys.take(window, cx);
            this.enter_pane(id, cx);
        });
    }

    /// The window shows the Settings tab this frame while `settings`. The Settings tab shown
    /// over a tab takes the keyboard from the pane that had it, which is no longer drawn; once
    /// it closes, the tab shown has it again if a pane had it as the Settings tab came.
    pub(super) fn keys_follow_settings(
        &mut self,
        settings: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        keyboard::key_state(window);
        match (!settings, self.keys.left) {
            (false, None) => {
                let held = self.workspace.locate(|pane| self.has_keyboard(pane, window, cx));
                self.keys.left = Some(!self.focus.is_focused(window) && held.is_some());
            }
            (true, Some(had)) => {
                self.keys.left = None;
                if had && self.keys.to.is_none() {
                    self.keys.to = self.shown_tab_key().map(|tab| (tab, false));
                }
            }
            _ => {}
        }
    }
}

/// Where a pane of `kind` comes among a tab's panes when the tab shown hands the keyboard to
/// the first that takes it, the pane the person was in aside (`Shell::tab_takes_keys`): its
/// terminals first, then its chats, its pages and its devices. A file, the diffs and the agent
/// desktop take it only as the pane the person was in.
fn takes_keys_after(kind: PaneKind) -> Option<u8> {
    match kind {
        PaneKind::Terminal => Some(0),
        PaneKind::Chat => Some(1),
        PaneKind::Browser => Some(2),
        PaneKind::Device => Some(3),
        PaneKind::Editor | PaneKind::Desktop => None,
    }
}

/// What has the keyboard when `pane` has it: its page, or GPUI's view for anything else.
fn surface_of(pane: &Pane, cx: &App) -> Surface {
    match pane.view() {
        Some(browser) => Surface::Page(browser.read(cx).page_id()),
        None => Surface::Gpui(Part::Pane(pane.id)),
    }
}

/// The pane among `panes` showing the page known by `page`, its own or its `DevTools`', and
/// the page's view.
fn page_in(panes: &[&Pane], page: u64, cx: &App) -> Option<(u64, Entity<BrowserView>)> {
    let is = |view: &Entity<BrowserView>| view.read(cx).page_id() == page;
    let own = |pane: &&&Pane| pane.view().is_some_and(is);
    if let Some(pane) = panes.iter().find(own) {
        return Some((pane.id, pane.view()?.clone()));
    }
    panes.iter().find_map(|pane| {
        let tools = pane.view()?.read(cx).devtools()?.clone();
        is(&tools).then_some((pane.id, tools))
    })
}

#[cfg(test)]
mod tests;

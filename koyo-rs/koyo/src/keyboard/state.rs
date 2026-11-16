//! The holders themselves: what has the keyboard in each of Koyo's windows, and what takes it
//! away from what the person put it in. Pure: what the app hears goes in, the change it makes
//! comes out, for the log and for `AppKit`.

use std::collections::HashMap;

/// A page's pane, by the browser host's id of it.
pub type PaneId = u64;
/// A window, by GPUI's id of it.
pub type WindowKey = u64;

/// What has the keyboard in a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    /// GPUI's own view, and in it the part GPUI's focus is in.
    Gpui(Part),
    /// A page's own views, Chromium's: a pinned, loose or worktree page, or `DevTools`.
    Page(PaneId),
    /// A native menu, which takes the keys while it tracks them.
    Menu,
    /// Nothing in the window: another window has the keys (a dialog, a sheet, one of
    /// Chromium's), or another app.
    Away,
}

/// What the person put the keyboard in, in a window: what has it there whenever nothing has
/// taken it away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Gpui(Part),
    Page(PaneId),
}

impl From<Surface> for Holder {
    fn from(surface: Surface) -> Holder {
        match surface {
            Surface::Gpui(part) => Holder::Gpui(part),
            Surface::Page(pane) => Holder::Page(pane),
        }
    }
}

/// The part of Koyo's own that has GPUI's focus, as far as it's known, named by what it is and
/// never by what's in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The window's own keys: nothing is typed into.
    Window,
    /// A pane's own view, by the shell's id of the pane: its chat, its terminal, its file's
    /// editor, a device or the agent desktop.
    Pane(u64),
    /// The address or the find bar of the page in pane `PaneId`.
    Bar(PaneId),
    /// Somewhere else of GPUI's, or somewhere it didn't say: a dialog's field, a settings page.
    Other,
}

/// What the person asked for that puts the keyboard somewhere: theirs to say, so the holder
/// is set from it as it's asked, not from what GPUI's focus does after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// ⌘L: the page's address.
    Address,
    /// ⌥⌘I: a page's `DevTools` closed with the keyboard in them, which the page has again.
    DevTools,
    /// A new tab, from the launcher or a button.
    NewTab,
    /// ⌘⇧T: a closed tab back.
    Reopen,
    /// A link opened in a tab beside the page's.
    LinkBeside,
    /// ⌃⇥: the tab switcher's pick.
    Switcher,
    /// The launcher's pick of an open tab.
    Launcher,
    /// An app control client's `pane_focus`, which moves the person's keyboard as they would.
    PaneFocus,
}

/// Why the keyboard moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The person pressed the surface, as the window heard before handing the press on.
    Click,
    /// What the person asked for.
    Intent(Intent),
    /// GPUI's focus moved, as the app or the person moved it.
    Focused,
    /// Another tab is shown, and the keyboard is handed to it.
    TabShown,
    /// The window became the key window, or stopped being it.
    WindowKey,
    WindowResigned,
    /// A dialog of Koyo's opened over the window, or closed.
    DialogOpened,
    DialogClosed,
    /// The page that had it closed.
    PageClosed,
    /// A native menu began tracking the keys, or ended.
    MenuOpened,
    MenuClosed,
    /// Koyo became the app in front, or another app did.
    AppActive,
    AppInactive,
    /// A window of Koyo's that had the keyboard closed, and it falls back to this one.
    WindowClosed,
}

/// The keyboard moved in `window`, from `from` to `to`, because of `cause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub window: WindowKey,
    pub from: Holder,
    pub to: Holder,
    pub cause: Cause,
}

/// One window's keyboard.
#[derive(Clone, Debug)]
struct Held {
    /// What the person put it in.
    surface: Surface,
    /// Whether the window is the key window: if not, its keys go to another.
    key: bool,
    /// Whether a dialog of Koyo's is open over it, taking its keys.
    dialog: bool,
    /// How many native menus opened while it was the key window are tracking the keys.
    menus: u32,
    /// The tab it shows, by its key, as it was handed the keyboard last.
    tab: Option<u64>,
    /// What each tab it showed before had the keyboard in, as it was left.
    tabs: HashMap<u64, Surface>,
}

impl Held {
    /// A window as it opens: key, with its own keys.
    fn new() -> Held {
        Held {
            surface: Surface::Gpui(Part::Window),
            key: true,
            dialog: false,
            menus: 0,
            tab: None,
            tabs: HashMap::new(),
        }
    }

    /// What has the keyboard, with the app in front or not.
    fn holder(&self, inactive: bool) -> Holder {
        if self.menus > 0 {
            Holder::Menu
        } else if self.dialog || !self.key || inactive {
            Holder::Away
        } else {
            self.surface.into()
        }
    }
}

/// Every window's keyboard. A window is known from the first thing heard of it.
#[derive(Clone, Debug, Default)]
pub struct Keyboards {
    windows: HashMap<WindowKey, Held>,
    /// Whether another app is in front: no window of Koyo's has the keys then.
    inactive: bool,
}

impl Keyboards {
    /// What has the keyboard in `window`, if it's known.
    pub fn holder(&self, window: WindowKey) -> Option<Holder> {
        Some(self.windows.get(&window)?.holder(self.inactive))
    }

    /// What the person put the keyboard in, in `window`, if it's known.
    pub fn surface(&self, window: WindowKey) -> Option<Surface> {
        Some(self.windows.get(&window)?.surface)
    }

    /// Runs `with` on `window`'s keyboard, and says how its holder changed, if it did.
    fn change(
        &mut self,
        window: WindowKey,
        cause: Cause,
        with: impl FnOnce(&mut Held),
    ) -> Option<Change> {
        let inactive = self.inactive;
        let held = self.windows.entry(window).or_insert_with(Held::new);
        let from = held.holder(inactive);
        with(held);
        let to = held.holder(inactive);
        (from != to).then_some(Change {
            window,
            from,
            to,
            cause,
        })
    }

    /// The person's keyboard in `window` is in `surface` now, because of `cause`.
    pub fn set(&mut self, window: WindowKey, surface: Surface, cause: Cause) -> Option<Change> {
        self.change(window, cause, |held| held.surface = surface)
    }

    /// GPUI's focus left the page in `pane` for `part` of its own: the page has the keyboard
    /// no more, if it had. Heard after another page took it, it changes nothing: two pages'
    /// focus moves are heard in no set order.
    pub fn left_page(&mut self, window: WindowKey, pane: PaneId, part: Part) -> Option<Change> {
        let had = self.surface(window) == Some(Surface::Page(pane));
        had.then(|| self.set(window, Surface::Gpui(part), Cause::Focused))?
    }

    /// `window` became the key window, or stopped being it.
    pub fn key(&mut self, window: WindowKey, key: bool) -> Option<Change> {
        let cause = if key {
            Cause::WindowKey
        } else {
            Cause::WindowResigned
        };
        self.change(window, cause, |held| held.key = key)
    }

    /// A dialog of Koyo's opened over `window`, or closed.
    pub fn dialog(&mut self, window: WindowKey, open: bool) -> Option<Change> {
        let cause = if open {
            Cause::DialogOpened
        } else {
            Cause::DialogClosed
        };
        self.change(window, cause, |held| held.dialog = open)
    }

    /// `window` shows the tab known by `tab` now, and hands it the keyboard: what the tab
    /// shown before had it in is kept for that tab, and what this one had, if anything's kept,
    /// is returned. The tab shown already has what it has now.
    pub fn tab_shown(&mut self, window: WindowKey, tab: u64) -> Option<Surface> {
        let held = self.windows.entry(window).or_insert_with(Held::new);
        match held.tab.replace(tab) {
            Some(was) if was == tab => return Some(held.surface),
            Some(was) => {
                held.tabs.insert(was, held.surface);
            }
            None => {}
        }
        held.tabs.get(&tab).copied()
    }

    /// The page in `pane` closed: a window whose keyboard the person had put in it has it
    /// at the window's own keys, until GPUI's focus says where it is now, and no tab has it
    /// kept.
    pub fn page_closed(&mut self, pane: PaneId) -> Vec<Change> {
        for held in self.windows.values_mut() {
            held.tabs.retain(|_, surface| *surface != Surface::Page(pane));
        }
        let windows: Vec<WindowKey> = (self.windows.iter())
            .filter(|(_, held)| held.surface == Surface::Page(pane))
            .map(|(window, _)| *window)
            .collect();
        (windows.into_iter())
            .filter_map(|window| {
                self.set(window, Surface::Gpui(Part::Window), Cause::PageClosed)
            })
            .collect()
    }

    /// The person pressed `surface` in `window`.
    pub fn click(&mut self, window: WindowKey, surface: Surface) -> Option<Change> {
        self.set(window, surface, Cause::Click)
    }

    /// A native menu began tracking the keys, or ended: the key window's, while it's open.
    pub fn menu(&mut self, open: bool) -> Vec<Change> {
        let cause = if open {
            Cause::MenuOpened
        } else {
            Cause::MenuClosed
        };
        let windows: Vec<WindowKey> = self.windows.keys().copied().collect();
        (windows.into_iter())
            .filter_map(|window| {
                self.change(window, cause, |held| {
                    if open && held.key {
                        held.menus += 1;
                    } else if !open {
                        held.menus = held.menus.saturating_sub(1);
                    }
                })
            })
            .collect()
    }

    /// Koyo became the app in front, or another app did.
    pub fn active(&mut self, active: bool) -> Vec<Change> {
        let cause = if active {
            Cause::AppActive
        } else {
            Cause::AppInactive
        };
        let before: Vec<(WindowKey, Holder)> = (self.windows.iter())
            .map(|(window, held)| (*window, held.holder(self.inactive)))
            .collect();
        self.inactive = !active;
        (before.into_iter())
            .filter_map(|(window, from)| {
                let to = self.holder(window)?;
                (from != to).then_some(Change {
                    window,
                    from,
                    to,
                    cause,
                })
            })
            .collect()
    }

    /// A window of Koyo's closed: one the keyboard is kept for (`window`), which goes with its
    /// keyboard, or another (a dialog, a panel, one of Chromium's). Another that was the key
    /// window hands the keys back to one the keyboard is kept for, which has what the person
    /// had in it again: the one whose change is returned, to be made the key window.
    pub fn closed(&mut self, window: Option<WindowKey>, was_key: bool) -> Option<Change> {
        if let Some(window) = window {
            self.windows.remove(&window);
            return None;
        }
        if !was_key {
            return None;
        }
        let back = self.windows.keys().copied().min()?;
        self.change(back, Cause::WindowClosed, |held| {
            held.key = true;
            held.dialog = false;
        })
    }
}

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

/// Why the keyboard moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
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
}

impl Held {
    /// A window as it opens: key, with its own keys.
    fn new() -> Held {
        Held {
            surface: Surface::Gpui(Part::Window),
            key: true,
            dialog: false,
        }
    }

    fn holder(&self) -> Holder {
        if self.dialog || !self.key {
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
}

impl Keyboards {
    /// What has the keyboard in `window`, if it's known.
    #[cfg(test)]
    pub fn holder(&self, window: WindowKey) -> Option<Holder> {
        Some(self.windows.get(&window)?.holder())
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
        let held = self.windows.entry(window).or_insert_with(Held::new);
        let from = held.holder();
        with(held);
        let to = held.holder();
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

    /// The page in `pane` closed: a window whose keyboard the person had put in it has it
    /// at the window's own keys, until GPUI's focus says where it is now.
    pub fn page_closed(&mut self, pane: PaneId) -> Vec<Change> {
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
}

//! The new-tab launcher (`docs/new-tab.md`): ⌘T, or a tab strip's +, opens a floating bar over the
//! window instead of a tab. A pill in it says what kind of tab Enter opens, a browser tab, a chat
//! or a terminal, or a new worktree (`docs/arc-sidebar.md` 8), and Tab goes through them in that
//! order. What's typed is an address or a search,
//! a message for a new chat, or a command for a new terminal; with nothing typed, Enter opens an
//! empty tab of the kind. Under the field, what the history holds for it, filtered as it's typed,
//! and the tabs open already, as Zen's floating address bar lists them.
//!
//! ⌘L opens it on the page the person is in, as Arc's does: the page's address in the field, all
//! of it selected, and what's typed over it loads in that page rather than a new tab. It needs
//! a page: with none in the tab shown it opens nothing.

mod act;
mod here;
mod render;
mod rows;
mod suggest;
#[cfg(test)]
mod tests;
mod tone;
mod view;

use gpui_kit::assets::IconName;
use gpui_kit::{
    App, Context, Div, Hsla, InteractiveElement as _, KeyBinding, Window, actions, hsla,
};

#[cfg(test)]
pub use self::view::Launcher;
use crate::model::PaneKind;
use crate::shell::Shell;

actions!(
    launcher,
    [
        /// Opens the launcher over the window, or closes it when it's open.
        NewTab,
        /// The address of the page the person is in, to change: in the launcher opened on the
        /// page, or, in the Orca layout, in the page's own field.
        OpenAddress,
        NextKind,
        PreviousKind,
        RemoveFromHistory,
        AcceptSuggestion
    ]
);

/// ⌘T and ⌘L anywhere; and in the launcher's field, Tab and ⇧Tab through the kinds, ⇧⌫ to forget the row
/// picked, → at the end to take its text, and the keys that move through the rows, which the field
/// would otherwise take.
pub fn init(cx: &mut App) {
    let field = Some("Launcher > Input");
    cx.bind_keys([
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("cmd-l", OpenAddress, None),
        KeyBinding::new("tab", NextKind, field),
        KeyBinding::new("shift-tab", PreviousKind, field),
        KeyBinding::new("shift-delete", RemoveFromHistory, field),
        KeyBinding::new("shift-backspace", RemoveFromHistory, field),
        KeyBinding::new("right", AcceptSuggestion, field),
    ]);
    crate::list::bind_keys("Launcher > Input", cx);
}

/// The kinds of tab the launcher opens, in the order Tab goes through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Browser,
    Chat,
    Terminal,
    /// The agent desktop's tab: the one there is, shown, or a new one.
    Desktop,
    /// A device's tab, opened as the picker (`docs/devices.md` 8).
    Device,
    /// Not a tab: a worktree, named here and started from the branch the New Worktree dialog
    /// picks.
    Worktree,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Browser,
        Kind::Chat,
        Kind::Terminal,
        Kind::Desktop,
        Kind::Device,
        Kind::Worktree,
    ];

    /// The kind after this one, or before it, round the ends.
    pub fn step(self, by: isize) -> Kind {
        let at = Self::ALL.iter().position(|kind| *kind == self).unwrap_or(0);
        let len = Self::ALL.len() as isize;
        Self::ALL[(at as isize + by).rem_euclid(len) as usize]
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Browser => "Browser",
            Kind::Chat => "Chat",
            Kind::Terminal => "Terminal",
            Kind::Desktop => "Desktop",
            Kind::Device => "Device",
            Kind::Worktree => "Worktree",
        }
    }

    pub fn icon(self) -> IconName {
        self.pane().map_or(IconName::GitBranchPlus, PaneKind::icon)
    }

    /// The pill's colour, each kind's own so a glance tells them apart, and dark enough for its
    /// white label at 4.5:1 or better.
    pub fn color(self) -> Hsla {
        let (hue, saturation, lightness) = match self {
            Kind::Browser => (212., 0.85, 0.46),
            Kind::Chat => (265., 0.65, 0.55),
            Kind::Terminal => (152., 0.62, 0.31),
            Kind::Desktop => (190., 0.75, 0.3),
            Kind::Device => (330., 0.6, 0.42),
            Kind::Worktree => (28., 0.8, 0.4),
        };
        hsla(hue / 360., saturation, lightness, 1.)
    }

    /// What the field says with nothing in it.
    pub fn placeholder(self) -> &'static str {
        match self {
            Kind::Browser => "Search or enter address…",
            Kind::Chat => "Ask a new chat…",
            Kind::Terminal => "Run a command in a new terminal…",
            Kind::Desktop => "Open the agent desktop",
            Kind::Device => "Open an iPhone or iPad simulator",
            Kind::Worktree => "Name a new worktree…",
        }
    }

    /// The kind of pane it opens; a worktree opens none.
    pub fn pane(self) -> Option<PaneKind> {
        match self {
            Kind::Browser => Some(PaneKind::Browser),
            Kind::Chat => Some(PaneKind::Chat),
            Kind::Terminal => Some(PaneKind::Terminal),
            Kind::Desktop => Some(PaneKind::Desktop),
            Kind::Device => Some(PaneKind::Device),
            Kind::Worktree => None,
        }
    }
}

impl Shell {
    /// What the window does with the launcher's actions, wherever the keyboard is: opens it over
    /// the window on the selected checkout, for a new tab or, with a page shown, on that page.
    pub fn handle_launcher(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &NewTab, window, cx| this.open_launcher(window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenAddress, window, cx| this.open_address(window, cx)),
            )
    }

    /// Opens the launcher over the window, on the selected checkout; with none, there's nowhere
    /// for a tab to open, and it doesn't.
    pub fn open_launcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_launcher_on(None, window, cx);
    }

    /// ⌘L, or a click on the Arc sidebar's address: the address of the page shown, to change
    /// (`Shell::shown_page`). In Arc, where a page has no toolbar, the launcher opens on the
    /// page; in Orca the keyboard goes to the page's own field, all of it selected. With no
    /// page in the tab shown there's no address to change, and nothing opens: the key goes on
    /// to whatever has the keyboard, as one the window doesn't take.
    pub fn open_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = (self.shown_page(cx)).and_then(|view| Some((self.pane_of(&view)?.1, view)));
        let Some((pane, view)) = shown else {
            return cx.propagate();
        };
        if crate::settings::get(cx).layout == crate::settings::Layout::Orca {
            self.enter_pane(pane, cx);
            view.update(cx, |view, cx| view.focus_address(window, cx));
            return;
        }
        let address = view.read(cx).address();
        self.open_launcher_on(Some(view::Page { pane, address }), window, cx);
    }

    fn open_launcher_on(
        &mut self,
        page: Option<view::Page>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(here) = here::Here::of(self, cx) else {
            log::warn!("no checkout is selected to open a tab on");
            return;
        };
        let pages = self.pages_shown(cx);
        view::open(cx.entity().downgrade(), here, page, &pages, window, cx);
    }
}

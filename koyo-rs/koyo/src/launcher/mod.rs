//! The new-tab launcher (`docs/new-tab.md`): ⌘T, or a tab strip's +, opens a floating bar over the
//! window instead of a tab. A pill in it says what kind of tab Enter opens, a browser tab, a chat
//! or a terminal, and Tab goes through them in that order. What's typed is an address or a search,
//! a message for a new chat, or a command for a new terminal; with nothing typed, Enter opens an
//! empty tab of the kind. Under the field, what the history holds for it, filtered as it's typed,
//! and the tabs open already, as Zen's floating address bar lists them.

mod act;
mod here;
mod render;
mod rows;
mod suggest;
#[cfg(test)]
mod tests;
mod view;

use gpui_kit::assets::IconName;
use gpui_kit::{App, Context, Div, Hsla, InteractiveElement as _, KeyBinding, Window, actions, hsla};

#[cfg(test)]
pub use self::view::Launcher;
use crate::model::PaneKind;
use crate::shell::Shell;

actions!(
    launcher,
    [
        /// Opens the launcher over the window, or closes it when it's open.
        NewTab,
        NextKind,
        PreviousKind,
        RemoveFromHistory,
        AcceptSuggestion
    ]
);

/// ⌘T anywhere; and in the launcher's field, Tab and ⇧Tab through the kinds, ⇧⌫ to forget the row
/// picked, → at the end to take its text, and the keys that move through the rows, which the field
/// would otherwise take.
pub fn init(cx: &mut App) {
    let field = Some("Launcher > Input");
    cx.bind_keys([
        KeyBinding::new("cmd-t", NewTab, None),
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
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Browser, Kind::Chat, Kind::Terminal];

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
        }
    }

    pub fn icon(self) -> IconName {
        self.pane().icon()
    }

    /// The pill's colour, each kind's own so a glance tells them apart, and dark enough for its
    /// white label at 4.5:1 or better.
    pub fn color(self) -> Hsla {
        let (hue, saturation, lightness) = match self {
            Kind::Browser => (212., 0.85, 0.46),
            Kind::Chat => (265., 0.65, 0.55),
            Kind::Terminal => (152., 0.62, 0.31),
        };
        hsla(hue / 360., saturation, lightness, 1.)
    }

    /// What the field says with nothing in it.
    pub fn placeholder(self) -> &'static str {
        match self {
            Kind::Browser => "Search or enter address…",
            Kind::Chat => "Ask a new chat…",
            Kind::Terminal => "Run a command in a new terminal…",
        }
    }

    /// The kind of pane it opens.
    pub fn pane(self) -> PaneKind {
        match self {
            Kind::Browser => PaneKind::Browser,
            Kind::Chat => PaneKind::Chat,
            Kind::Terminal => PaneKind::Terminal,
        }
    }
}

impl Shell {
    /// What the window does with the launcher's action: opens it over the window on the selected
    /// checkout.
    pub fn handle_launcher(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &NewTab, window, cx| this.open_launcher(window, cx)))
    }

    /// Opens the launcher over the window, on the selected checkout; with none, there's nowhere
    /// for a tab to open, and it doesn't.
    pub fn open_launcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(here) = here::Here::of(self, cx) else {
            log::warn!("no checkout is selected to open a tab on");
            return;
        };
        view::open(cx.entity().downgrade(), here, window, cx);
    }
}

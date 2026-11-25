//! What the window shows where a tab goes, from the top down (`docs/sidebar.md` 11, 11.9):
//! the Settings tab while it's shown; else the pinned page shown, whichever project is; else the
//! loose page the space shows; else the selected checkout's tab. What's under the one in front
//! stays where it is, and shows once what's over it goes: `pinned_shown`, `loose_shown` and
//! `shown_checkout_tab` say where the person is under the Settings tab as over it, and what's
//! drawn and lit asks [`Workspace::front`].

use std::path::Path;

use super::{SettingsTab, Tab, Workspace};

/// What the window shows where a tab goes.
#[derive(Clone, Copy, Debug)]
pub enum Front<'a> {
    /// The Settings tab, over whatever is under it.
    Settings(&'a SettingsTab),
    /// The pinned page at this place among them.
    Pinned(usize),
    /// The loose page at this place among those the space shown lists.
    Loose(usize),
    /// The selected checkout's tab, as `(project, branch, tab)`.
    Checkout((usize, usize, usize)),
}

impl Front<'_> {
    /// The selected checkout's tab in front, as `(project, branch, tab)`, while one is.
    pub fn checkout(self) -> Option<(usize, usize, usize)> {
        match self {
            Front::Checkout(at) => Some(at),
            Front::Settings(_) | Front::Pinned(_) | Front::Loose(_) => None,
        }
    }

    /// The place of the loose page in front among those the space shown lists, while one is.
    pub fn loose(self) -> Option<usize> {
        match self {
            Front::Loose(at) => Some(at),
            Front::Settings(_) | Front::Pinned(_) | Front::Checkout(_) => None,
        }
    }
}

impl Workspace {
    /// What the window shows where a tab goes: none while no project is shown and neither a
    /// pinned nor a loose page is.
    pub fn front(&self) -> Option<Front<'_>> {
        if let Some(settings) = self.settings_shown() {
            return Some(Front::Settings(settings));
        }
        if let Some(at) = self.pinned_shown() {
            return Some(Front::Pinned(at));
        }
        if let Some(at) = self.loose_shown() {
            return Some(Front::Loose(at));
        }
        self.shown_tab().map(Front::Checkout)
    }

    /// The tab shown: the one in front, unless that's the Settings tab.
    pub fn tab_shown(&self) -> Option<&Tab> {
        match self.front()? {
            Front::Settings(_) => None,
            Front::Pinned(at) => Some(&self.apart.pins.tabs.get(at)?.tab),
            Front::Loose(at) => Some(&self.apart.listed_page(at)?.tab),
            Front::Checkout(at) => self.tab(at),
        }
    }

    /// The tab shown, to change.
    pub fn tab_shown_mut(&mut self) -> Option<&mut Tab> {
        match self.front()? {
            Front::Settings(_) => None,
            Front::Pinned(at) => self.pinned_tab_mut(at),
            Front::Loose(at) => self.apart.listed_tab_mut(at),
            Front::Checkout(at) => self.tab_mut(at),
        }
    }

    /// The project whose profile the pinned or the loose page shown keeps, if one is shown.
    pub fn shown_apart_project(&self) -> Option<&Path> {
        match self.front()? {
            Front::Pinned(at) => Some(&self.apart.pins.tabs.get(at)?.project),
            Front::Loose(at) => Some(&self.apart.listed_page(at)?.project),
            Front::Settings(_) | Front::Checkout(_) => None,
        }
    }
}

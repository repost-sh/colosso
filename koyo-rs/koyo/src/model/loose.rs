//! Loose pages (`docs/arc-sidebar.md` 11): pages open in a space that are no project's, listed
//! under the pinned pages and over the open project. Each is filed under a space; a space lists
//! its own, or every space's while the settings share them (`settings::LoosePages`), in the
//! order of their rows. Its own, a space shows again the one it showed last when it's come back
//! to; shared, the one shown stays shown as the space changes, as a pinned one does. A loose
//! page shown is the tab shown in place of the selected checkout's, until a tab of a checkout
//! is; a pinned page shown is shown over both, and the Settings tab, listed after the loose
//! pages, over all of them (`settings_tab.rs`). Only pages are loose; one's `DevTools`, detached,
//! split its tab as they split any page's. A page's tab moves between a checkout's tabs, the
//! loose pages and the pinned ones.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::pinned::Pins;
use super::settings_tab::Settings;
use super::{Pane, Pinned, Tab, Workspace};

/// A loose page.
#[derive(Clone, Debug)]
pub struct Loose {
    /// Its page's pane, first, and its `DevTools`' beside it while they're detached.
    pub tab: Tab,
    /// The project whose profile its page keeps, its cookies and sign-ins: the one it was
    /// dragged out of, or what stands for none (`browser::NO_PROJECT`) for a page opened outside
    /// any project.
    pub project: PathBuf,
}

impl Loose {
    /// What its row is known by wherever it moves: its page's pane.
    pub fn key(&self) -> u64 {
        self.tab.first().id
    }
}

/// A loose page with the space it's filed under: the one it was opened in or brought to, or
/// that took it from a space deleted.
#[derive(Clone, Debug)]
struct Filed {
    space: u64,
    page: Loose,
}

/// The tabs that are no checkout's: the pinned pages, common to every space, the loose ones,
/// each filed under a space, and the Settings tab. A space lists those filed under it, or every
/// one while they're shared; which it lists is only a view, so sharing them and not loses or
/// moves none.
#[derive(Clone, Debug, Default)]
pub(super) struct Apart {
    pub(super) pins: Pins,
    pub(super) settings: Settings,
    /// Every loose page, in the order of their rows in any space that lists them.
    loose: Vec<Filed>,
    /// The loose page each space shows in place of the selected checkout's tab, by the space's
    /// id: its page's key. Shared, a space may show one filed under another.
    shown: BTreeMap<u64, u64>,
    /// The space shown, by its id.
    space: u64,
    /// Whether every space lists every loose page.
    shared: bool,
}

impl Apart {
    /// The loose pages the space shown lists, in order, with their places among them all.
    fn listed(&self) -> impl Iterator<Item = (usize, &Filed)> {
        let lists = move |filed: &Filed| self.shared || filed.space == self.space;
        (self.loose.iter().enumerate()).filter(move |(_, filed)| lists(filed))
    }

    /// The place among them all of the listed page at `at`, or the one after the last listed
    /// when there's none there: where a page put at `at` goes.
    fn place(&self, at: usize) -> usize {
        let mut listed = self.listed().map(|(place, _)| place);
        let last = listed.by_ref().take(at).last();
        listed
            .next()
            .unwrap_or_else(|| last.map_or(self.loose.len(), |last| last + 1))
    }

    /// The place among those listed of the one the space shown shows.
    fn shown_at(&self) -> Option<usize> {
        let key = *self.shown.get(&self.space)?;
        self.listed().position(|(_, filed)| filed.page.key() == key)
    }

    /// The space shown shows the listed page at `at`. False when there's none there.
    fn show(&mut self, at: usize) -> bool {
        let key = self.listed().nth(at).map(|(_, filed)| filed.page.key());
        if let Some(key) = key {
            self.shown.insert(self.space, key);
            self.pins.leave();
            self.settings.close();
        }
        key.is_some()
    }

    /// Takes the listed page at `at` out, and no space shows it. Shown by the space shown, the
    /// one before it shows in its place when `closed`, or the one after when it was the first;
    /// taken elsewhere, none does.
    fn take(&mut self, at: usize, closed: bool) -> Option<Loose> {
        let (place, _) = self.listed().nth(at)?;
        let shown = self.shown_at() == Some(at);
        let taken = self.loose.remove(place).page;
        self.shown.retain(|_, key| *key != taken.key());
        let next = self.listed().nth(at.saturating_sub(1));
        if let Some((_, next)) = next.filter(|_| shown && closed) {
            self.shown.insert(self.space, next.page.key());
        }
        Some(taken)
    }

    /// The listed page at `at`.
    pub(super) fn listed_page(&self, at: usize) -> Option<&Loose> {
        self.listed().nth(at).map(|(_, filed)| &filed.page)
    }

    /// The tab of the listed page at `at`, to change.
    pub(super) fn listed_tab_mut(&mut self, at: usize) -> Option<&mut Tab> {
        let (place, _) = self.listed().nth(at)?;
        Some(&mut self.loose.get_mut(place)?.page.tab)
    }

    /// The tab shown in place of the selected checkout's, if a pinned or a loose page is, the
    /// Settings tab over it or not: the pinned page shown, else the loose page the space shown
    /// shows.
    pub(super) fn shown_tab(&self) -> Option<&Tab> {
        let pinned = || Some(&self.pins.tabs.get(self.pins.shown()?)?.tab);
        let loose = || Some(&self.listed_page(self.shown_at()?)?.tab);
        pinned().or_else(loose)
    }

    /// Neither the Settings tab, a pinned page nor a loose one is shown from now on: the
    /// selected checkout's tab is.
    pub(super) fn leave(&mut self) {
        self.pins.leave();
        self.settings.close();
        self.shown.remove(&self.space);
    }

    /// Every page that's no checkout's, with the project whose profile it keeps: the pinned,
    /// then the loose ones.
    fn pages(&self) -> impl Iterator<Item = (&Tab, &Path)> {
        let pinned = (self.pins.tabs.iter()).map(|pinned| (&pinned.tab, pinned.project.as_path()));
        let loose = self.loose.iter().map(|filed| &filed.page);
        pinned.chain(loose.map(|page| (&page.tab, page.project.as_path())))
    }

    fn tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        let pinned = self.pins.tabs.iter_mut().map(|pinned| &mut pinned.tab);
        let loose = self.loose.iter_mut().map(|filed| &mut filed.page.tab);
        pinned.chain(loose)
    }
}

impl Workspace {
    /// The space known by `space` is the one shown: its loose pages are the ones listed, and
    /// the one it showed last shows again. Shared, the loose page shown stays shown there, as a
    /// pinned one does, and with none shown, its project's tab shows. Whether that's another
    /// space than was shown.
    pub fn show_pages_of(&mut self, space: u64) -> bool {
        let apart = &mut self.apart;
        let changed = apart.space != space;
        if changed && apart.shared {
            match apart.shown.get(&apart.space).copied() {
                Some(key) => apart.shown.insert(space, key),
                None => apart.shown.remove(&space),
            };
        }
        apart.space = space;
        changed
    }

    /// Every space lists every loose page from now on when `shared`, else each the ones filed
    /// under it. Whether that's a change.
    pub fn share_loose(&mut self, shared: bool) -> bool {
        let changed = self.apart.shared != shared;
        self.apart.shared = shared;
        changed
    }

    /// Whether every space lists every loose page.
    pub fn loose_shared(&self) -> bool {
        self.apart.shared
    }

    /// Whether the page shown stays shown whichever space is: a pinned page, or a loose one
    /// while every space shares them.
    pub fn shown_in_every_space(&self) -> bool {
        self.apart.pins.shown().is_some() || (self.apart.shared && self.loose_shown().is_some())
    }

    /// Whether the space shown lists the loose pages filed under the space known by `space`:
    /// its own, or every space's while they're shared.
    pub fn lists_loose_of(&self, space: u64) -> bool {
        self.apart.shared || space == self.apart.space
    }

    /// The space shown, by its id.
    pub fn space_shown(&self) -> u64 {
        self.apart.space
    }

    /// The loose pages the space shown lists, in the order of their rows.
    pub fn loose(&self) -> Vec<&Loose> {
        self.apart.listed().map(|(_, filed)| &filed.page).collect()
    }

    /// Every loose page, with the space it's filed under, in the order of their rows: what the
    /// window keeps.
    pub fn loose_filed(&self) -> impl Iterator<Item = (u64, &Loose)> {
        (self.apart.loose.iter()).map(|filed| (filed.space, &filed.page))
    }

    /// The loose page each space shows, as the space's id and the page's key.
    pub fn loose_shown_by(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.apart.shown.iter().map(|(space, key)| (*space, *key))
    }

    /// The loose page that's the tab shown, by its place among those listed: none while a
    /// pinned page is shown over it. The Settings tab over it, it's still this one.
    pub fn loose_shown(&self) -> Option<usize> {
        let over = self.apart.pins.shown().is_some();
        self.apart.shown_at().filter(|_| !over)
    }

    /// Where the loose page known by `key` is among those the space shown lists.
    pub fn loose_at(&self, key: u64) -> Option<usize> {
        (self.apart.listed()).position(|(_, filed)| filed.page.key() == key)
    }

    /// Adds `page` to the loose pages, filed under the space shown, at `place` among those it
    /// lists or after them, shown when `shown`, in place of whatever was: where it is among
    /// them.
    pub fn add_loose(&mut self, page: Loose, place: usize, shown: bool) -> usize {
        let key = page.key();
        let space = self.apart.space;
        let at = self.apart.place(place);
        self.apart.loose.insert(at, Filed { space, page });
        let listed = self.loose_at(key).unwrap_or_default();
        if shown {
            self.apart.show(listed);
        }
        listed
    }

    /// Adds `pages` after every loose page, filed under the space known by `space`, which shows
    /// the one at `shown`: as the window brings them back.
    pub fn bring_back_loose(&mut self, space: u64, pages: Vec<Loose>, shown: Option<usize>) {
        if let Some(page) = shown.and_then(|at| pages.get(at)) {
            self.apart.shown.insert(space, page.key());
        }
        let filed = pages.into_iter().map(|page| Filed { space, page });
        self.apart.loose.extend(filed);
    }

    /// The space known by `space` shows the loose page known by `key` once it's shown, as it
    /// did when the window was kept: one filed under another space, which shared pages allow.
    pub fn bring_back_loose_shown(&mut self, space: u64, key: u64) {
        if self.apart.loose.iter().any(|filed| filed.page.key() == key) {
            self.apart.shown.insert(space, key);
        }
    }

    /// Shows the loose page at `at` of those listed, in place of the selected checkout's tab
    /// or a pinned page. False when there's none there.
    pub fn show_loose(&mut self, at: usize) -> bool {
        self.apart.show(at)
    }

    /// Moves the loose page in place `from` to place `to` among those listed, the others
    /// closing up; the one shown stays shown, and it stays filed where it was. False when
    /// there's no such place.
    pub fn move_loose(&mut self, from: usize, to: usize) -> bool {
        let Some((place, _)) = self.apart.listed().nth(from) else {
            return false;
        };
        let moved = self.apart.loose.remove(place);
        let to = self.apart.place(to);
        self.apart.loose.insert(to, moved);
        true
    }

    /// Closes the loose page at `at` of those listed, handing it back: shown, the one before
    /// it shows in its place, the one after when it was first, and the selected checkout's tab
    /// when it was the only one.
    pub fn close_loose(&mut self, at: usize) -> Option<Loose> {
        self.apart.take(at, true)
    }

    /// Takes every loose page filed under the space known by `from`, which is gone: they're
    /// filed under the space known by `to`, after every page, or handed back to close when
    /// there's none to take them.
    pub fn hand_on_loose(&mut self, from: u64, to: Option<u64>) -> Vec<Loose> {
        let apart = &mut self.apart;
        let (gone, kept) = (std::mem::take(&mut apart.loose).into_iter())
            .partition::<Vec<_>, _>(|filed| filed.space == from);
        apart.loose = kept;
        apart.shown.remove(&from);
        match to {
            Some(to) if to != from => {
                let handed = gone.into_iter().map(|filed| Filed { space: to, ..filed });
                apart.loose.extend(handed);
                Vec::new()
            }
            _ => {
                let pages: Vec<Loose> = gone.into_iter().map(|filed| filed.page).collect();
                (apart.shown).retain(|_, key| pages.iter().all(|page| page.key() != *key));
                pages
            }
        }
    }

    /// Moves the tab at `at`, as `(project, branch, tab)`, among the loose pages the space shown
    /// lists, at `place` or after them, filed under that space, and shows it there. False, and
    /// nothing changes, unless its single pane is a page.
    pub fn loosen_tab(&mut self, at: (usize, usize, usize), place: usize) -> bool {
        let Some((tab, project)) = self.take_page_tab(at) else {
            return false;
        };
        self.add_loose(Loose { tab, project }, place, true);
        true
    }

    /// Moves the loose page at `at` of those listed to the checkout `to`, as
    /// `(project, branch)`, at `place` among its tabs or after them, and shows it there. False,
    /// and nothing changes, when either isn't there.
    pub fn settle_loose(&mut self, at: usize, to: (usize, usize), place: usize) -> bool {
        self.settle(to, place, |workspace| {
            Some(workspace.apart.take(at, false)?.tab)
        })
    }

    /// Pins the loose page at `at` of those listed at the address `home`, in slot `place` among
    /// the pages pinned already or after them: it goes on showing if it was the tab shown.
    /// Where it is among the pinned.
    pub fn pin_loose(&mut self, at: usize, home: String, place: usize) -> Option<usize> {
        let shown = self.loose_shown() == Some(at);
        let page = self.apart.take(at, false)?;
        let pinned = Pinned {
            tab: page.tab,
            home,
            project: page.project,
            more: serde_json::Map::new(),
        };
        Some(self.add_pinned(pinned, place, shown))
    }

    /// Unpins the page at `at` among the pinned: it's a loose page of the space shown, at
    /// `place` among those listed or after them, and shows there. False when there's none
    /// there.
    pub fn loosen_pinned(&mut self, at: usize, place: usize) -> bool {
        let Some(unpinned) = self.remove_pinned(at) else {
            return false;
        };
        let page = Loose {
            tab: unpinned.tab,
            project: unpinned.project,
        };
        self.add_loose(page, place, true);
        true
    }

    /// The first pane that's no checkout's, pinned or loose in any space, that is `found`: its
    /// id, and the project whose profile its page keeps.
    pub fn find_apart(&self, found: impl Fn(&Pane) -> bool) -> Option<(u64, &Path)> {
        self.apart.pages().find_map(|(tab, project)| {
            let pane = tab.panes().into_iter().find(|pane| found(pane))?;
            Some((pane.id, project))
        })
    }

    /// Every tab there is: the pinned, each space's loose ones, and every checkout's.
    fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.everywhere().map(|visited| visited.tab)
    }

    /// Every tab there is, to change.
    fn tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        let Workspace {
            apart, projects, ..
        } = self;
        let open = (projects.iter_mut())
            .flat_map(|project| &mut project.branches)
            .flat_map(|unit| &mut unit.tabs);
        apart.tabs_mut().chain(open)
    }

    /// Pane `id`, on whichever checkout it is, among the pinned or among any space's loose
    /// pages.
    pub fn pane(&self, id: u64) -> Option<&Pane> {
        self.tab_holding(id)?
            .panes()
            .into_iter()
            .find(|pane| pane.id == id)
    }

    /// Pane `id`, wherever it is, to change.
    pub fn pane_mut(&mut self, id: u64) -> Option<&mut Pane> {
        self.pane_mut_where(|pane| pane.id == id)
    }

    /// The first pane there is, wherever it is, that is `found`, to change.
    pub fn pane_mut_where(&mut self, found: impl Fn(&Pane) -> bool) -> Option<&mut Pane> {
        let mut panes = self.tabs_mut().flat_map(Tab::panes_mut);
        panes.find(|pane| found(pane))
    }

    /// The tab holding pane `id`, on whichever checkout it is, among the pinned or among any
    /// space's loose pages.
    pub fn tab_holding(&self, id: u64) -> Option<&Tab> {
        self.tabs().find(|tab| tab.has_pane(id))
    }

    /// The tab holding pane `id`, wherever it is, to change.
    pub fn tab_holding_mut(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs_mut().find(|tab| tab.has_pane(id))
    }
}

#[cfg(test)]
mod tests;

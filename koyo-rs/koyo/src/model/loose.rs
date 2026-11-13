//! Loose pages (`docs/arc-sidebar.md` 11): pages open in a space that are no project's, listed
//! under the pinned pages and over the open project. Each space has its own, in the order of
//! their rows, and shows again the one it showed last when it's come back to. A loose page shown
//! is the tab shown in place of the selected checkout's, until a tab of a checkout is; a pinned
//! page shown is shown over both. Only pages are loose; one's `DevTools`, detached, split its tab
//! as they split any page's. A page's tab moves between a checkout's tabs, the loose pages and
//! the pinned ones.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::pinned::Pins;
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

/// A space's loose pages, and the one it shows.
#[derive(Clone, Debug, Default)]
struct SpacePages {
    pages: Vec<Loose>,
    /// The one shown in place of the selected checkout's tab, by its place among them.
    shown: Option<usize>,
}

impl SpacePages {
    fn shown(&self) -> Option<usize> {
        self.shown.filter(|at| *at < self.pages.len())
    }

    /// Takes the page at `at` out. Shown, the one before it shows in its place when `closed`,
    /// or the one after when it was the first; taken elsewhere, none does.
    fn take(&mut self, at: usize, closed: bool) -> Option<Loose> {
        if at >= self.pages.len() {
            return None;
        }
        let shown = self.shown();
        let taken = self.pages.remove(at);
        self.shown = match shown {
            Some(shown) if shown == at && closed && !self.pages.is_empty() => {
                Some(at.saturating_sub(1))
            }
            Some(shown) if shown == at => None,
            Some(shown) if shown > at => Some(shown - 1),
            shown => shown,
        };
        Some(taken)
    }
}

/// The pages that are no checkout's: the pinned ones, common to every space, and each space's
/// loose ones, by the space's id.
#[derive(Clone, Debug, Default)]
pub(super) struct Apart {
    pub(super) pins: Pins,
    spaces: BTreeMap<u64, SpacePages>,
    /// The space shown, by its id.
    space: u64,
}

impl Apart {
    fn here(&self) -> Option<&SpacePages> {
        self.spaces.get(&self.space)
    }

    fn here_mut(&mut self) -> &mut SpacePages {
        self.spaces.entry(self.space).or_default()
    }

    /// The page shown in place of the selected checkout's tab, with the project whose profile
    /// it keeps: the pinned page shown, else the loose page the space shown shows.
    fn shown(&self) -> Option<(&Tab, &Path)> {
        let pinned = || {
            let pinned = self.pins.tabs.get(self.pins.shown()?)?;
            Some((&pinned.tab, pinned.project.as_path()))
        };
        let loose = || {
            let here = self.here()?;
            let page = here.pages.get(here.shown()?)?;
            Some((&page.tab, page.project.as_path()))
        };
        pinned().or_else(loose)
    }

    /// The tab shown in place of the selected checkout's, if a pinned or a loose page is.
    pub(super) fn shown_tab(&self) -> Option<&Tab> {
        self.shown().map(|(tab, _)| tab)
    }

    /// Neither a pinned page nor a loose one is shown from now on: the selected checkout's tab
    /// is.
    pub(super) fn leave(&mut self) {
        self.pins.leave();
        if let Some(here) = self.spaces.get_mut(&self.space) {
            here.shown = None;
        }
    }

    /// Every page that's no checkout's, with the project whose profile it keeps: the pinned,
    /// then each space's loose ones.
    fn pages(&self) -> impl Iterator<Item = (&Tab, &Path)> {
        let pinned = (self.pins.tabs.iter()).map(|pinned| (&pinned.tab, pinned.project.as_path()));
        let loose = self.spaces.values().flat_map(|space| &space.pages);
        pinned.chain(loose.map(|page| (&page.tab, page.project.as_path())))
    }

    fn tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        let pinned = self.pins.tabs.iter_mut().map(|pinned| &mut pinned.tab);
        let loose = self.spaces.values_mut().flat_map(|space| &mut space.pages);
        pinned.chain(loose.map(|page| &mut page.tab))
    }
}

impl Workspace {
    /// The space known by `space` is the one shown: its loose pages are the ones listed, and
    /// the one it showed last shows again. Whether that's another space than was shown.
    pub fn show_pages_of(&mut self, space: u64) -> bool {
        let changed = self.apart.space != space;
        self.apart.space = space;
        changed
    }

    /// The loose pages of the space shown, in the order of their rows.
    pub fn loose(&self) -> &[Loose] {
        self.apart.here().map_or(&[], |here| &here.pages)
    }

    /// Each space's loose pages, by its id, with the place of the one it shows: what the
    /// window keeps.
    pub fn loose_by_space(&self) -> impl Iterator<Item = (u64, &[Loose], Option<usize>)> {
        (self.apart.spaces.iter())
            .map(|(space, here)| (*space, here.pages.as_slice(), here.shown()))
    }

    /// The loose page that's the tab shown, by its place among the space's: none while a pinned
    /// page is shown over it.
    pub fn loose_shown(&self) -> Option<usize> {
        let over = self.apart.pins.shown().is_some();
        self.apart.here()?.shown().filter(|_| !over)
    }

    /// Where the loose page known by `key` is among the space shown's.
    pub fn loose_at(&self, key: u64) -> Option<usize> {
        self.loose().iter().position(|page| page.key() == key)
    }

    /// Adds `page` to the space shown's loose pages at `place`, or after them, shown when
    /// `shown`, in place of whatever was: where it is among them.
    pub fn add_loose(&mut self, page: Loose, place: usize, shown: bool) -> usize {
        let here = self.apart.here_mut();
        let was = here.shown().map(|at| here.pages[at].key());
        let place = place.min(here.pages.len());
        here.pages.insert(place, page);
        here.shown = was.and_then(|key| here.pages.iter().position(|page| page.key() == key));
        if shown {
            here.shown = Some(place);
            self.apart.pins.leave();
        }
        place
    }

    /// Adds `pages` after the loose pages of the space known by `space`, the one at `shown`
    /// shown: as the window brings them back.
    pub fn bring_back_loose(&mut self, space: u64, pages: Vec<Loose>, shown: Option<usize>) {
        let here = self.apart.spaces.entry(space).or_default();
        let before = here.pages.len();
        here.pages.extend(pages);
        here.shown = shown.map(|at| before + at).or(here.shown);
    }

    /// Shows the loose page at `at` of the space shown, in place of the selected checkout's tab
    /// or a pinned page. False when there's none there.
    pub fn show_loose(&mut self, at: usize) -> bool {
        let here = self.apart.here_mut();
        let there = at < here.pages.len();
        if there {
            here.shown = Some(at);
            self.apart.pins.leave();
        }
        there
    }

    /// Moves the loose page in place `from` to place `to` among the space shown's, the others
    /// closing up; the one shown stays shown. False when there's no such place.
    pub fn move_loose(&mut self, from: usize, to: usize) -> bool {
        let here = self.apart.here_mut();
        if from >= here.pages.len() {
            return false;
        }
        let shown = here.shown().map(|at| here.pages[at].key());
        let moved = here.pages.remove(from);
        let to = to.min(here.pages.len());
        here.pages.insert(to, moved);
        here.shown = shown.and_then(|key| here.pages.iter().position(|page| page.key() == key));
        true
    }

    /// Closes the loose page at `at` of the space shown, handing it back: shown, the one before
    /// it shows in its place, the one after when it was first, and the selected checkout's tab
    /// when it was the only one.
    pub fn close_loose(&mut self, at: usize) -> Option<Loose> {
        self.apart.here_mut().take(at, true)
    }

    /// Takes every loose page of the space known by `from`, which is gone: they're the last of
    /// the space known by `to`'s, or handed back to close when there's none to take them.
    pub fn hand_on_loose(&mut self, from: u64, to: Option<u64>) -> Vec<Loose> {
        let gone = self.apart.spaces.remove(&from).unwrap_or_default();
        match to {
            Some(to) if to != from => {
                self.bring_back_loose(to, gone.pages, None);
                Vec::new()
            }
            _ => gone.pages,
        }
    }

    /// Moves the tab at `at`, as `(project, branch, tab)`, among the space shown's loose pages,
    /// at `place` or after them, and shows it there. False, and nothing changes, unless its
    /// single pane is a page.
    pub fn loosen_tab(&mut self, at: (usize, usize, usize), place: usize) -> bool {
        let Some((tab, project)) = self.take_page_tab(at) else {
            return false;
        };
        self.add_loose(Loose { tab, project }, place, true);
        true
    }

    /// Moves the loose page at `at` of the space shown to the checkout `to`, as
    /// `(project, branch)`, at `place` among its tabs or after them, and shows it there. False,
    /// and nothing changes, when either isn't there.
    pub fn settle_loose(&mut self, at: usize, to: (usize, usize), place: usize) -> bool {
        self.settle(to, place, |workspace| {
            Some(workspace.apart.here_mut().take(at, false)?.tab)
        })
    }

    /// Pins the loose page at `at` of the space shown at the address `home`, after the pages
    /// pinned already: it goes on showing if it was the tab shown. Where it is among the
    /// pinned.
    pub fn pin_loose(&mut self, at: usize, home: String) -> Option<usize> {
        let shown = self.loose_shown() == Some(at);
        let page = self.apart.here_mut().take(at, false)?;
        let pinned = Pinned {
            tab: page.tab,
            home,
            project: page.project,
            more: serde_json::Map::new(),
        };
        Some(self.add_pinned(pinned, shown))
    }

    /// Unpins the page at `at` among the pinned: it's a loose page of the space shown, at
    /// `place` among them or after them, and shows there. False when there's none there.
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

    /// The project whose profile the pinned or the loose page shown keeps, if one is shown.
    pub fn shown_apart_project(&self) -> Option<&Path> {
        self.apart.shown().map(|(_, project)| project)
    }

    /// Every tab there is: the pinned, each space's loose ones, and every checkout's.
    fn tabs(&self) -> impl Iterator<Item = &Tab> {
        let open = (self.projects.iter())
            .flat_map(|project| &project.branches)
            .flat_map(|unit| &unit.tabs);
        self.apart.pages().map(|(tab, _)| tab).chain(open)
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

    /// The tab shown, to change: the pinned page shown, else the space's loose page shown, else
    /// the selected checkout's.
    pub fn tab_shown_mut(&mut self) -> Option<&mut Tab> {
        if let Some(shown) = self.apart_shown_key() {
            return self.apart.tabs_mut().find(|tab| tab.first().id == shown);
        }
        let unit = self.branch_mut()?;
        unit.tabs.get_mut(unit.active_tab)
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

//! Panes the window has one of, the agent desktop's and each device's: the one that's open,
//! wherever it is, or a new one in a tab of its own at a [`Place`]. Whoever opens it names
//! the place: the person's menus on the selected checkout, an agent's tool where the agent
//! is (`control/exec/beside.rs`).

use gpui_kit::Context;

use super::Shell;
use crate::model::{Pane, Tab};

/// Where the tab of such a pane opens, and whether one that's open already is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// On the selected checkout, shown; one open already is shown where it is.
    Shown,
    /// Out of sight, right after the tab at `(project, branch, tab)`, the tab its checkout
    /// shows left shown; with no tab there, as on a checkout with none, it's the one that
    /// checkout shows. `None`, there's no checkout to open on. One open already is left as
    /// it is.
    After(Option<(usize, usize, usize)>),
}

impl Shell {
    /// The place on the selected checkout: shown, or out of sight after the tab it shows.
    pub(crate) fn place(&self, shown: bool) -> Place {
        if shown {
            Place::Shown
        } else {
            Place::After(self.workspace.shown_tab())
        }
    }

    /// The pane `open` found, shown when `place` says; else the one `make` makes, in a tab
    /// of its own at `place`. `None` with no checkout to open it on.
    pub(crate) fn one_pane(
        &mut self,
        open: Option<((usize, usize, usize), u64)>,
        place: Place,
        make: impl FnOnce(&mut Self, &mut Context<Self>) -> Pane,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        if let Some(((project, branch, tab), pane)) = open {
            if place == Place::Shown {
                self.workspace.select_tab(project, branch, tab);
                cx.notify();
            }
            return Some(pane);
        }
        let after = match place {
            Place::Shown => None,
            Place::After(after) => Some(after?),
        };
        let pane = make(self, cx);
        let id = pane.id;
        let tab = Tab::Single(pane);
        match after {
            None => self.workspace.push_tab(tab),
            Some(at) if self.workspace.tab(at).is_some() => {
                self.workspace.insert_tab_after(at, tab);
            }
            // No tab there to come after, as on a checkout with none: it's the one shown.
            Some((project, branch, _)) => {
                let unit = self.workspace.unit_mut(project, branch)?;
                unit.tabs.push(tab);
                unit.active_tab = unit.tabs.len() - 1;
            }
        }
        cx.notify();
        (self.workspace.find_pane(|pane| pane.id == id)).map(|_| id)
    }
}

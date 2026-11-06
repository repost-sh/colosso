//! What the browser tabs tell the history (`docs/new-tab.md` 4.1): each page a tab loads and how
//! it came there, its title as it changes, the page's icon, and the searches typed in an address,
//! each in the checkout its tab is on.

use gpui_kit::{Context, Entity};

use super::Shell;
use crate::browser::{BrowserView, Visit};
use crate::history::{self, Place};

impl Shell {
    /// `view` loaded a page: a visit of its checkout's, unless it came back or forward or is the
    /// page the tab showed already, which only retitles it.
    pub(super) fn note_visit(
        &mut self,
        view: &Entity<BrowserView>,
        visit: &Visit,
        cx: &mut Context<Self>,
    ) {
        match visit.how {
            Some(how) => {
                if let Some(place) = self.tab_of(view).and_then(|at| self.place_at(at)) {
                    history::visited(&visit.url, &visit.title, how, &place, cx);
                }
            }
            None => history::retitled(&visit.url, &visit.title, cx),
        }
    }

    /// `view` changed: the page it shows, loaded, keeps the title and icon it has now.
    pub(super) fn note_page(&mut self, view: &Entity<BrowserView>, cx: &mut Context<Self>) {
        let view = view.read(cx);
        let Some((url, title)) = view.loaded_page() else {
            return;
        };
        let (url, title, icon) = (url.to_owned(), title.to_owned(), view.favicon());
        history::retitled(&url, &title, cx);
        if let Some(icon) = icon {
            history::keep_icon(&url, &icon, cx);
        }
    }

    /// The person searched the web for `words` from `view`'s address.
    pub(super) fn note_search(
        &mut self,
        view: &Entity<BrowserView>,
        words: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(place) = self.tab_of(view).and_then(|at| self.place_at(at)) {
            history::used(history::Kind::Search, words, &place, cx);
        }
    }

    /// The page browser pane `pane` opens first was asked for by name, in the launcher.
    pub(crate) fn mark_typed(&mut self, pane: u64, cx: &mut Context<Self>) {
        let view = (self.workspace.branch()).and_then(|unit| unit.pane(pane)?.browser.clone());
        if let Some(view) = view {
            view.update(cx, |view, _| view.mark_typed());
        }
    }

    /// Where the tab at `(project, branch, tab)` is.
    fn place_at(&self, (project, branch, _): (usize, usize, usize)) -> Option<Place> {
        let project = self.workspace.projects.get(project)?;
        let unit = project.branches.get(branch)?;
        Some(Place {
            worktree: unit.worktree.path.clone(),
            project: project.path.clone(),
        })
    }
}

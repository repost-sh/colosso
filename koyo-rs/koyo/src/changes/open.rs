//! The changes' diffs in a workspace tab, one on each checkout: a file picked in the changes
//! panel's list opens it right after the tab shown, or shows it where it is, scrolled to the
//! file; and ⌘⇧T brings it back once closed.

use gpui_kit::{AppContext as _, Context, Entity, Window};

use super::{ChangesEvent, ChangesPanel, ChangesTab};
use crate::model::{Pane, Tab};
use crate::navigation::Route;
use crate::shell::Shell;

impl Shell {
    /// What the changes panel asks: the diffs shown, at the file picked in its list, the
    /// keyboard staying in the list to go on moving through it.
    pub(crate) fn changes_event(
        &mut self,
        panel: &Entity<ChangesPanel>,
        event: &ChangesEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ChangesEvent::Picked = event;
        self.show_changes(panel, cx);
    }

    /// Shows the selected checkout's Changes tab, opening it on `panel`'s diffs right after
    /// the tab shown if it isn't open.
    pub(crate) fn show_changes(&mut self, panel: &Entity<ChangesPanel>, cx: &mut Context<Self>) {
        let Some((project, branch, _)) = self.workspace.shown_tab() else {
            return;
        };
        let unit = &self.workspace.projects[project].branches[branch];
        let open = (unit.tabs.iter())
            .position(|tab| tab.panes().iter().any(|pane| pane.changes.is_some()));
        if let Some(at) = open {
            self.workspace.select_tab(project, branch, at);
        } else {
            let changes = cx.new(|cx| ChangesTab::new(panel.clone(), cx));
            self.workspace
                .open_beside_shown(Tab::Single(Pane::changes(changes)));
        }
        // The diffs are something to look at, so leave settings for them.
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    /// The panes `ids` of the tab back at `at`, as `(project, branch, tab)`, which is shown,
    /// show the checkout's diffs again.
    pub(crate) fn reopen_changes(
        &mut self,
        at: (usize, usize, usize),
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }
        // The checkout it's on is selected now; its panels are made after this change.
        self.follow_selection(window, cx);
        let Some(panel) = self.changes_panel(window, cx) else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(at) else {
            return;
        };
        for pane in tab
            .panes_mut()
            .into_iter()
            .filter(|pane| ids.contains(&pane.id))
        {
            pane.changes = Some(cx.new(|cx| ChangesTab::new(panel.clone(), cx)));
        }
    }
}

#[cfg(test)]
mod tests;

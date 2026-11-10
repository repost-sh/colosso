//! The pane the person is in: the one they pressed in or gave the keyboard last. It's kept on
//! the pane, so it stays theirs while the sidebar, a dialog or another window has the keyboard,
//! and goes with the pane wherever it's moved.

use super::Workspace;

impl Workspace {
    /// The person is in pane `id` from now on, as its tab's `current` answers. Whether that's
    /// news: false when it was its tab's current pane already, or no such pane is open.
    pub fn enter_pane(&mut self, id: u64) -> bool {
        let Some((at, _)) = self.find_pane(|pane| pane.id == id) else {
            return false;
        };
        let next = self.entered + 1;
        let Some(tab) = self.tab_mut(at) else {
            return false;
        };
        if tab.current().id == id {
            return false;
        }
        if let Some(pane) = tab.panes_mut().into_iter().find(|pane| pane.id == id) {
            pane.entered_at = next;
        }
        self.entered = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::model::tests::workspace;
    use crate::model::{Pane, PaneKind, Tab, Workspace};

    /// One project with its main checkout, shown, and a tab of `count` chat panes side by side:
    /// the workspace and the panes' ids, left to right.
    fn split(count: usize) -> (Workspace, Vec<u64>) {
        let mut workspace = workspace(&[]);
        let panes: Vec<Pane> = (0..count).map(|_| Pane::new(PaneKind::Chat)).collect();
        let ids = panes.iter().map(|pane| pane.id).collect();
        let tab = (panes.into_iter().map(Tab::Single)).reduce(Tab::beside);
        workspace.push_tab(tab.expect("a pane"));
        (workspace, ids)
    }

    fn current(workspace: &Workspace) -> u64 {
        workspace.tab_shown().expect("a tab").current().id
    }

    #[test]
    fn a_tabs_current_pane_is_its_first_until_the_person_is_in_another() {
        let (mut workspace, ids) = split(3);
        assert_eq!(current(&workspace), ids[0]);
        assert!(!workspace.enter_pane(ids[0]), "in it already");
        assert!(workspace.enter_pane(ids[2]));
        assert_eq!(current(&workspace), ids[2]);
        assert!(!workspace.enter_pane(ids[2]), "no news the second time");
        assert!(workspace.enter_pane(ids[1]));
        assert_eq!(current(&workspace), ids[1]);
        assert!(!workspace.enter_pane(ids[1] + 1000), "no such pane");
    }

    #[test]
    fn the_pane_before_takes_over_when_the_current_one_closes_and_it_goes_with_a_pane_moved() {
        let (mut workspace, ids) = split(3);
        workspace.enter_pane(ids[1]);
        workspace.enter_pane(ids[2]);
        let unit = workspace.branch_mut().expect("a checkout");
        unit.close_pane(ids[2]);
        assert_eq!(current(&workspace), ids[1], "the one they were in before");
        // Taken into a tab of its own, the pane they were in leaves the first to the tab.
        let unit = workspace.branch_mut().expect("a checkout");
        assert!(unit.pane_to_tab(ids[1], 1));
        assert_eq!(unit.tabs[0].current().id, ids[0]);
        assert_eq!(unit.tabs[1].current().id, ids[1]);
    }
}

//! Moving tabs and panes on a checkout: a tab dropped on a side of a pane splits with it,
//! a pane dragged out of its split goes to another pane or a tab of its own, and tabs
//! reorder in the strip.

use super::{BranchUnit, Edge, Tab};

impl BranchUnit {
    /// The tab holding pane `id`.
    pub fn tab_with(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.has_pane(id))
    }

    /// Puts the tab at `from` at `edge` of pane `target`, in whichever tab holds it, and
    /// shows that tab; the tab moved leaves the strip. False when there's no such tab or
    /// pane, or the pane is in the tab moved.
    pub fn drop_tab(&mut self, from: usize, target: u64, edge: Edge) -> bool {
        let Some(to) = self.tab_with(target) else {
            return false;
        };
        if from == to || from >= self.tabs.len() {
            return false;
        }
        let incoming = self.tabs.remove(from);
        let to = if from < to { to - 1 } else { to };
        // The tab at `to` holds `target`, so the tab dropped always finds its place there.
        self.tabs[to].place(target, edge, incoming);
        self.active_tab = to;
        true
    }

    /// Moves pane `id` to `edge` of pane `target`, and shows the tab it's in then. A tab's
    /// only pane moves with its tab.
    pub fn move_pane(&mut self, id: u64, target: u64, edge: Edge) -> bool {
        let (Some(from), Some(to)) = (self.tab_with(id), self.tab_with(target)) else {
            return false;
        };
        if id == target {
            return false;
        }
        if matches!(self.tabs[from], Tab::Single(_)) {
            return self.drop_tab(from, target, edge);
        }
        let Some(pane) = self.tabs[from].take(id) else {
            return false;
        };
        self.tabs[to].place(target, edge, Tab::Single(pane));
        self.active_tab = to;
        true
    }

    /// Takes pane `id` out of its split into a tab of its own at `at` in the strip, and
    /// shows it; a tab's only pane moves its tab there.
    pub fn pane_to_tab(&mut self, id: u64, at: usize) -> bool {
        let Some(from) = self.tab_with(id) else {
            return false;
        };
        if matches!(self.tabs[from], Tab::Single(_)) {
            self.move_tab(from, at);
            return true;
        }
        let Some(pane) = self.tabs[from].take(id) else {
            return false;
        };
        let at = at.min(self.tabs.len());
        self.tabs.insert(at, Tab::Single(pane));
        self.active_tab = at;
        true
    }

    /// Moves the tab at `from` to where the tab at `to` is, and shows it.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(from);
        let to = to.min(self.tabs.len());
        self.tabs.insert(to, tab);
        self.active_tab = to;
    }

    /// Closes pane `id`: out of its split, or with its tab when it's the tab's only pane.
    pub fn close_pane(&mut self, id: u64) {
        let Some(at) = self.tab_with(id) else {
            return;
        };
        if self.tabs[at].take(id).is_none() {
            self.close(at);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{BranchUnit, Edge, Pane, PaneKind, Tab};

    /// A checkout with `count` chat tabs, the last one shown, and their panes' ids.
    fn unit(count: usize) -> (BranchUnit, Vec<u64>) {
        let worktree = colosso_git::Worktree {
            path: "/work/moves".into(),
            head: colosso_git::Head::Detached("abc".into()),
            is_main: true,
        };
        let mut unit = BranchUnit::new(worktree);
        let panes: Vec<Pane> = (0..count).map(|_| Pane::new(PaneKind::Chat)).collect();
        let ids = panes.iter().map(|pane| pane.id).collect();
        unit.tabs = panes.into_iter().map(Tab::Single).collect();
        unit.active_tab = count - 1;
        (unit, ids)
    }

    fn layout(unit: &BranchUnit) -> Vec<Vec<u64>> {
        let tabs = unit.tabs.iter();
        tabs.map(|tab| tab.panes().iter().map(|pane| pane.id).collect())
            .collect()
    }

    #[test]
    fn a_tab_dropped_on_a_pane_splits_with_it_and_leaves_the_strip() {
        let (mut unit, ids) = unit(3);
        assert!(unit.drop_tab(0, ids[2], Edge::Right));
        assert_eq!(layout(&unit), [vec![ids[1]], vec![ids[2], ids[0]]]);
        assert_eq!(unit.active_tab, 1, "the split shows");
        assert!(!unit.drop_tab(1, ids[0], Edge::Left), "not on itself");
    }

    #[test]
    fn a_pane_moves_between_splits_and_out_into_a_tab_of_its_own() {
        let (mut unit, ids) = unit(3);
        unit.drop_tab(1, ids[0], Edge::Bottom);
        // [a / b], [c]
        assert!(unit.move_pane(ids[1], ids[2], Edge::Left));
        assert_eq!(layout(&unit), [vec![ids[0]], vec![ids[1], ids[2]]]);
        assert_eq!(unit.active_tab, 1);
        assert!(unit.pane_to_tab(ids[2], 0));
        assert_eq!(layout(&unit), [vec![ids[2]], vec![ids[0]], vec![ids[1]]]);
        assert_eq!(unit.active_tab, 0);
    }

    #[test]
    fn closing_a_pane_closes_its_tab_only_when_it_was_alone() {
        let (mut unit, ids) = unit(3);
        unit.drop_tab(2, ids[1], Edge::Top);
        unit.close_pane(ids[2]);
        assert_eq!(layout(&unit), [vec![ids[0]], vec![ids[1]]]);
        unit.close_pane(ids[1]);
        assert_eq!(layout(&unit), [vec![ids[0]]]);
        unit.move_tab(0, 5);
        assert_eq!(layout(&unit), [vec![ids[0]]]);
    }
}

//! Moving tabs and panes on a checkout: a tab dropped on a side of a pane splits with it,
//! a pane dragged out of its split goes to another pane or a tab of its own, a tab dropped
//! on another joins it side by side, and tabs reorder in the strip. And a tab dragged from
//! one checkout's list in the Arc sidebar to another's (`docs/arc-sidebar.md` 5).

use super::{BranchUnit, Edge, Pane, Tab, Workspace};

impl Workspace {
    /// Moves the tab at `from`, as `(project, branch, tab)`, to the checkout `to`, as
    /// `(project, branch)`, at `at` among its tabs, and shows it there. False when it's on that
    /// checkout already, or either isn't there.
    pub fn move_tab_to_checkout(
        &mut self,
        (project, branch, tab): (usize, usize, usize),
        to: (usize, usize),
        at: usize,
    ) -> bool {
        if (project, branch) == to || self.unit_mut(to.0, to.1).is_none() {
            return false;
        }
        let Some(moved) = (self.unit_mut(project, branch)).and_then(|unit| unit.close(tab)) else {
            return false;
        };
        let Some(unit) = self.unit_mut(to.0, to.1) else {
            return false;
        };
        let at = at.min(unit.tabs.len());
        unit.tabs.insert(at, moved);
        self.select_tab(to.0, to.1, at);
        true
    }
}

impl Workspace {
    /// Moves a project among those of `group`, a space's, given by their places in the order
    /// its tiles show them: the one in slot `from` goes to slot `to`, the others of the group
    /// closing up, and every project outside it keeps its place. The project selected stays
    /// selected. False, and nothing changes, when there's no such slot or it's the same one.
    pub fn move_project_among(&mut self, group: &[usize], from: usize, to: usize) -> bool {
        let known = group.iter().all(|place| *place < self.projects.len());
        if from == to || from >= group.len() || to >= group.len() || !known {
            return false;
        }
        let selected = self.project().map(|project| project.path.clone());
        let mut order = group.to_vec();
        let moved = order.remove(from);
        order.insert(to, moved);
        // The group's places in the list, which its projects fill again in their new order.
        let mut places = group.to_vec();
        places.sort_unstable();
        let mut projects: Vec<_> = std::mem::take(&mut self.projects)
            .into_iter()
            .map(Some)
            .collect();
        let moving: Vec<_> = (order.iter())
            .map(|place| projects[*place].take())
            .collect();
        for (place, project) in places.into_iter().zip(moving) {
            projects[place] = project;
        }
        self.projects = projects.into_iter().flatten().collect();
        let reselected = selected.and_then(|path| self.project_index(&path));
        self.select_project(reselected);
        true
    }
}

impl BranchUnit {
    /// The tab holding pane `id`.
    pub fn tab_with(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.has_pane(id))
    }

    /// Pane `id`, in whichever tab holds it.
    pub fn pane(&self, id: u64) -> Option<&Pane> {
        let tab = &self.tabs[self.tab_with(id)?];
        tab.panes().into_iter().find(|pane| pane.id == id)
    }

    /// Pane `id`, in whichever tab holds it, to change.
    pub fn pane_mut(&mut self, id: u64) -> Option<&mut Pane> {
        let at = self.tab_with(id)?;
        self.tabs[at]
            .panes_mut()
            .into_iter()
            .find(|pane| pane.id == id)
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

    /// Joins the tab at `from` to the tab at `onto`, side by side, on its right when
    /// `after`, every column sharing the room evenly, and shows them; the tab joined leaves
    /// the strip. False when either is missing, or they're the same.
    pub fn join_tabs(&mut self, from: usize, onto: usize, after: bool) -> bool {
        let count = self.tabs.len();
        if from == onto || from >= count || onto >= count {
            return false;
        }
        let incoming = self.tabs.remove(from);
        let onto = if from < onto { onto - 1 } else { onto };
        let here = self.tabs.remove(onto);
        let row = if after {
            Tab::beside(here, incoming)
        } else {
            Tab::beside(incoming, here)
        };
        self.tabs.insert(onto, row);
        self.active_tab = onto;
        true
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
    use crate::model::{BranchUnit, Edge, Pane, PaneKind, Tab, Workspace};

    #[test]
    fn a_project_moves_among_its_spaces_and_the_others_keep_their_places() {
        let paths: Vec<std::path::PathBuf> = (["a", "x", "b", "c", "y"].iter())
            .map(|name| format!("/src/{name}").into())
            .collect();
        let mut workspace = Workspace::new(&paths);
        workspace.select_project(Some(2));
        let names = |workspace: &Workspace| -> Vec<String> {
            (workspace.projects.iter())
                .map(|project| project.name.to_string())
                .collect()
        };
        // A space holds a, b and c; x and y are another's.
        let space = [0, 2, 3];
        assert!(workspace.move_project_among(&space, 0, 2));
        assert_eq!(names(&workspace), ["b", "x", "c", "a", "y"]);
        let selected = workspace.project().map(|project| project.name.to_string());
        assert_eq!(selected.as_deref(), Some("b"), "the same project, moved");
        assert!(workspace.move_project_among(&space, 2, 0));
        assert_eq!(names(&workspace), ["a", "x", "b", "c", "y"]);
        assert!(!workspace.move_project_among(&space, 1, 1), "where it is");
        assert!(!workspace.move_project_among(&space, 1, 3), "no such slot");
        assert!(
            !workspace.move_project_among(&[0, 9], 0, 1),
            "no such project"
        );
        assert_eq!(names(&workspace), ["a", "x", "b", "c", "y"]);
    }

    /// A checkout with `count` chat tabs, the last one shown, and their panes' ids.
    fn unit(count: usize) -> (BranchUnit, Vec<u64>) {
        let worktree = koyo_git::Worktree {
            path: "/work/moves".into(),
            head: koyo_git::Head::Detached("abc".into()),
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
    fn a_tab_dropped_on_another_joins_it_side_by_side_on_the_side_it_was_held() {
        let (mut unit, ids) = unit(4);
        assert!(unit.join_tabs(0, 2, true));
        assert_eq!(
            layout(&unit),
            [vec![ids[1]], vec![ids[2], ids[0]], vec![ids[3]]]
        );
        assert_eq!(unit.active_tab, 1, "the pair shows");
        // Joining a row adds a column to it rather than splitting one of its sides.
        assert!(unit.join_tabs(2, 1, false));
        assert_eq!(layout(&unit), [vec![ids[1]], vec![ids[3], ids[2], ids[0]]]);
        let Tab::Split(row) = &unit.tabs[1] else {
            panic!("a split");
        };
        assert!(
            (row.ratio - 1. / 3.).abs() < f32::EPSILON,
            "columns share evenly"
        );
        assert!(!unit.join_tabs(1, 1, true), "not with itself");
        assert!(
            !unit.join_tabs(0, 5, true),
            "not with a tab that isn't there"
        );
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

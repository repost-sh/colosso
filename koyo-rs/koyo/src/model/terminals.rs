//! Terminal panes on the checkouts: each shows a terminal by the number `crate::terminals`
//! knows it by, in a tab of its own or on a side of a split, and stays that terminal
//! wherever it moves.

use std::path::Path;

use gpui_kit::SharedString;

use super::{Pane, PaneKind, Tab, Workspace};

impl Workspace {
    /// Adds a tab for terminal `id` on the checkout at `worktree`, shown when `select`: its
    /// pane, unless no such checkout is listed.
    pub fn add_terminal_tab(
        &mut self,
        worktree: &Path,
        id: u64,
        title: SharedString,
        select: bool,
    ) -> Option<u64> {
        let pane = Pane {
            title,
            terminal: Some(id),
            ..Pane::new(PaneKind::Terminal)
        };
        self.add_pane_tab(worktree, pane, select)
    }

    /// Adds a tab of `pane` on the checkout at `worktree`, shown when `select`: its id, unless
    /// no such checkout is listed.
    pub(super) fn add_pane_tab(
        &mut self,
        worktree: &Path,
        pane: Pane,
        select: bool,
    ) -> Option<u64> {
        let (project, branch) = self.unit_at(worktree)?;
        let unit = &mut self.projects[project].branches[branch];
        let pane_id = pane.id;
        unit.tabs.push(Tab::Single(pane));
        if select {
            unit.active_tab = unit.tabs.len() - 1;
            self.select_branch(project, branch);
        }
        Some(pane_id)
    }

    /// Every terminal pane's terminal, with its checkout, split or not.
    pub fn terminal_tabs(&self) -> impl Iterator<Item = (&Path, u64)> {
        self.projects.iter().flat_map(|project| {
            project.branches.iter().flat_map(|unit| {
                (unit.tabs.iter().flat_map(Tab::panes)).filter_map(move |pane| {
                    pane.terminal.map(|id| (unit.worktree.path.as_path(), id))
                })
            })
        })
    }

    /// The terminals of the tabs closed lately, which ⌘⇧T may open again.
    pub fn closed_terminals(&self) -> impl Iterator<Item = u64> {
        (self.closed.iter())
            .flat_map(|closed| closed.tab.panes())
            .filter_map(|pane| pane.terminal)
    }

    /// Takes terminal `id`'s pane away, out of its split or with its tab, wherever it is.
    pub fn close_terminal_pane(&mut self, id: u64) {
        for unit in self
            .projects
            .iter_mut()
            .flat_map(|project| &mut project.branches)
        {
            let pane = (unit.tabs.iter().flat_map(Tab::panes))
                .find(|pane| pane.terminal == Some(id))
                .map(|pane| pane.id);
            if let Some(pane) = pane {
                unit.close_pane(pane);
                return;
            }
        }
    }

    /// Names terminal `id`'s pane `title`; whether that changed it.
    pub fn retitle_terminal(&mut self, id: u64, title: SharedString) -> bool {
        for unit in self
            .projects
            .iter_mut()
            .flat_map(|project| &mut project.branches)
        {
            for pane in unit.tabs.iter_mut().flat_map(Tab::panes_mut) {
                if pane.terminal == Some(id) {
                    if pane.title == title {
                        return false;
                    }
                    pane.title = title;
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use koyo_git::{Head, Worktree};

    use crate::model::{Edge, Pane, PaneKind, Tab, Workspace};

    /// One project with its main checkout, which has one chat tab, shown.
    fn workspace() -> Workspace {
        let mut workspace = Workspace::new(&["/src/app".into()]);
        let main = Worktree {
            path: "/src/app".into(),
            head: Head::Branch("main".into()),
            is_main: true,
        };
        workspace.set_worktrees(0, vec![main], &[]);
        workspace.select_worktree(0, "/src/app".as_ref());
        workspace.new_tab(PaneKind::Chat);
        workspace
    }

    #[test]
    fn a_terminal_split_beside_a_chat_is_listed_renamed_and_closed_alone() {
        let mut workspace = workspace();
        workspace.add_terminal_tab("/src/app".as_ref(), 7, "zsh".into(), false);
        let unit = workspace.branch_mut().unwrap();
        let chat = unit.tabs[0].panes()[0].id;
        assert!(
            unit.drop_tab(1, chat, Edge::Right),
            "the terminal's tab splits the chat"
        );
        assert_eq!(unit.tabs.len(), 1);

        let listed: Vec<_> = workspace.terminal_tabs().map(|(_, id)| id).collect();
        assert_eq!(listed, [7]);
        assert!(workspace.retitle_terminal(7, "vim".into()));
        assert!(
            !workspace.retitle_terminal(7, "vim".into()),
            "the same name changes nothing"
        );

        workspace.close_terminal_pane(7);
        let unit = workspace.branch().unwrap();
        assert!(matches!(
            &unit.tabs[..],
            [Tab::Single(Pane {
                kind: PaneKind::Chat,
                ..
            })]
        ));
        assert_eq!(workspace.terminal_tabs().count(), 0);
    }

    #[test]
    fn a_terminal_tab_goes_to_its_own_checkout_and_shows_when_asked() {
        let mut workspace = workspace();
        workspace.add_terminal_tab("/elsewhere".as_ref(), 1, "zsh".into(), true);
        assert_eq!(workspace.terminal_tabs().count(), 0, "no such checkout");
        workspace.add_terminal_tab("/src/app".as_ref(), 2, "zsh".into(), true);
        let unit = workspace.branch().unwrap();
        assert_eq!(unit.active_tab, 1);
        assert_eq!(unit.tabs[1].panes()[0].terminal, Some(2));
    }
}

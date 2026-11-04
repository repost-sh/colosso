//! Chat panes on the checkouts: each shows a thread by its id once it has one, in a tab of its
//! own or on a side of a split, and stays that thread wherever it moves.

use std::path::Path;

use koyo_agent::protocol::ids::ThreadId;

use super::{BranchUnit, Pane, PaneKind, Tab, Workspace};

impl Workspace {
    /// Adds a tab for thread `id` on the checkout at `worktree`, shown when `select`: its pane,
    /// unless no such checkout is listed. Its title comes from `retitle_chats`.
    pub fn add_chat_tab(&mut self, worktree: &Path, id: ThreadId, select: bool) -> Option<u64> {
        let pane = Pane {
            chat: Some(id),
            ..Pane::new(PaneKind::Chat)
        };
        self.add_pane_tab(worktree, pane, select)
    }

    /// Every chat pane's id and the thread it shows, if it has one yet, split or not.
    pub fn chat_panes(&self) -> impl Iterator<Item = (u64, Option<&ThreadId>)> {
        (self.projects.iter())
            .flat_map(|project| &project.branches)
            .flat_map(|unit| unit.tabs.iter().flat_map(Tab::panes))
            .filter(|pane| pane.kind == PaneKind::Chat)
            .map(|pane| (pane.id, pane.chat.as_ref()))
    }

    /// Pane `pane` shows thread `id` from now on.
    pub fn set_chat(&mut self, pane: u64, id: ThreadId) {
        if let Some(shown) = self.units_mut().find_map(|unit| unit.pane_mut(pane)) {
            shown.chat = Some(id);
        }
    }

    /// Takes away the panes showing thread `id`, out of their splits or with their tabs.
    pub fn close_chat_panes(&mut self, id: &ThreadId) {
        for unit in self.units_mut() {
            let panes: Vec<u64> = (unit.tabs.iter().flat_map(Tab::panes))
                .filter(|pane| pane.chat.as_ref() == Some(id))
                .map(|pane| pane.id)
                .collect();
            for pane in panes {
                unit.close_pane(pane);
            }
        }
    }

    /// Names each chat pane what `title_of` says its thread is called, if anything.
    pub fn retitle_chats<'a>(&mut self, title_of: impl Fn(&ThreadId) -> Option<&'a str>) {
        let panes = (self.units_mut()).flat_map(|unit| unit.tabs.iter_mut().flat_map(Tab::panes_mut));
        for pane in panes {
            let title = pane.chat.as_ref().and_then(&title_of);
            if let Some(title) = title.filter(|title| *title != pane.title.as_ref()) {
                pane.title = title.to_owned().into();
            }
        }
    }

    fn units_mut(&mut self) -> impl Iterator<Item = &mut BranchUnit> {
        self.projects.iter_mut().flat_map(|project| &mut project.branches)
    }
}

#[cfg(test)]
mod tests {
    use koyo_agent::protocol::ids::ThreadId;
    use koyo_git::{Head, Worktree};

    use crate::model::{Pane, PaneKind, Tab, Workspace};

    /// One project with its main checkout, shown, with no tabs.
    fn workspace() -> Workspace {
        let mut workspace = Workspace::new(&["/src/app".into()]);
        let main = Worktree {
            path: "/src/app".into(),
            head: Head::Branch("main".into()),
            is_main: true,
        };
        workspace.set_worktrees(0, vec![main]);
        workspace.select_worktree(0, "/src/app".as_ref());
        workspace
    }

    fn thread(id: &str) -> ThreadId {
        ThreadId(id.into())
    }

    fn shown(workspace: &Workspace) -> Vec<Option<&ThreadId>> {
        workspace.chat_panes().map(|(_, thread)| thread).collect()
    }

    #[test]
    fn a_chat_tab_goes_on_its_checkout_unshown_and_only_on_one_that_is_listed() {
        let mut workspace = workspace();
        workspace.new_tab(PaneKind::Chat);
        assert!(workspace.add_chat_tab("/src/app".as_ref(), thread("t"), false).is_some());
        let unit = workspace.branch().unwrap();
        assert_eq!(unit.tabs.len(), 2);
        assert_eq!(unit.active_tab, 0);
        assert_eq!(unit.tabs[1].first().chat, Some(thread("t")));
        assert!(workspace.add_chat_tab("/src/gone".as_ref(), thread("u"), false).is_none());
    }

    #[test]
    fn a_new_chat_has_no_thread_until_it_is_given_one() {
        let mut workspace = workspace();
        workspace.new_tab(PaneKind::Chat);
        let (pane, _) = workspace.chat_panes().next().unwrap();
        assert_eq!(shown(&workspace), [None]);
        workspace.set_chat(pane, thread("t"));
        assert_eq!(shown(&workspace), [Some(&thread("t"))]);
    }

    #[test]
    fn a_chat_is_found_in_a_split_and_its_panes_go_with_it() {
        let mut workspace = workspace();
        workspace.add_chat_tab("/src/app".as_ref(), thread("t"), false);
        let terminal = Pane::new(PaneKind::Terminal);
        let unit = workspace.branch_mut().unwrap();
        let Some(Tab::Single(chat)) = unit.tabs.pop() else {
            panic!("the chat's tab is one pane");
        };
        unit.tabs
            .push(Tab::beside(Tab::Single(terminal.clone()), Tab::Single(chat)));
        assert_eq!(shown(&workspace), [Some(&thread("t"))]);
        let title = |workspace: &Workspace| {
            let panes = workspace.branch().unwrap().tabs[0].panes();
            panes.iter().find(|pane| pane.chat.is_some()).unwrap().title.clone()
        };
        workspace.retitle_chats(|_| Some("How"));
        assert_eq!(title(&workspace), "How");
        workspace.retitle_chats(|_| None);
        assert_eq!(title(&workspace), "How");
        workspace.close_chat_panes(&thread("t"));
        assert!(shown(&workspace).is_empty());
        // What it shared the tab with stays.
        let unit = workspace.branch().unwrap();
        assert_eq!(unit.tabs[0].first().id, terminal.id);
    }
}

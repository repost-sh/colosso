//! Browser panes on the checkouts that have no view yet: tabs brought back at launch, each
//! holding the address its page opens at once the tab is shown (`docs/browser.md` 5).

use std::path::Path;

use gpui_kit::SharedString;

use super::{Pane, Unloaded, Workspace};

impl Workspace {
    /// Adds a tab after the checkout's last that opens `unloaded` once shown, titled `title`,
    /// without showing it: its pane, unless no checkout at `worktree` is listed.
    pub fn add_unloaded_tab(
        &mut self,
        worktree: &Path,
        unloaded: Unloaded,
        title: SharedString,
    ) -> Option<u64> {
        self.add_pane_tab(worktree, Pane::unloaded(unloaded, title), false)
    }
}

#[cfg(test)]
mod tests {
    use koyo_git::{Head, Worktree};

    use crate::model::{PaneKind, Unloaded, Workspace};

    fn unloaded(address: &str) -> Unloaded {
        Unloaded {
            address: address.into(),
            favicon: None,
        }
    }

    #[test]
    fn an_unloaded_tab_goes_last_on_its_checkout_unshown_and_only_on_one_that_is_listed() {
        let mut workspace = Workspace::new(&["/src/app".into()]);
        let main = Worktree {
            path: "/src/app".into(),
            head: Head::Branch("main".into()),
            is_main: true,
        };
        workspace.set_worktrees(0, vec![main]);
        workspace.new_tab(PaneKind::Chat);
        workspace.new_tab(PaneKind::Chat);
        workspace.select_tab(0, 0, 0);
        let pane = workspace.add_unloaded_tab(
            "/src/app".as_ref(),
            unloaded("https://example.com/"),
            "Example".into(),
        );
        let unit = workspace.branch().unwrap();
        assert_eq!(unit.tabs.len(), 3);
        assert_eq!(unit.active_tab, 0, "the tab shown stays shown");
        let added = unit.tabs[2].first();
        assert_eq!(Some(added.id), pane);
        assert_eq!(added.kind, PaneKind::Browser);
        assert_eq!(added.title, "Example");
        assert!(added.browser.is_none(), "no page until it's shown");
        let address = added
            .unloaded
            .as_ref()
            .map(|unloaded| unloaded.address.as_str());
        assert_eq!(address, Some("https://example.com/"));
        let gone = workspace.add_unloaded_tab(
            "/src/gone".as_ref(),
            unloaded("https://example.org/"),
            "Gone".into(),
        );
        assert!(gone.is_none());
    }
}

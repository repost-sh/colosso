//! Showing the tab ⌃⇥ picked wherever it is now (`docs/tab-switcher.md` 2): as a press on its
//! row or its tile does, whichever project and space the window shows meanwhile.

use gpui_kit::Context;

use crate::shell::Shell;

impl Shell {
    /// Shows the tab known by `key`, its first pane's, if it's still open, as a press on its
    /// row or its tile does (`show_tab_holding`): a worktree's tab in its project, which the
    /// shell follows into its space, a pinned page over the space shown, a loose page in its
    /// space; and it takes the keyboard, its file if it shows one.
    pub(super) fn show_visited(&mut self, key: u64, cx: &mut Context<Self>) {
        let tab = self.workspace.tab_holding(key);
        let file = tab.and_then(|tab| tab.panes().into_iter().find(|pane| pane.file.is_some()));
        if let Some(file) = file.map(|pane| pane.id) {
            self.workspace.enter_pane(file);
        }
        self.intend(crate::keyboard::Intent::Switcher);
        self.show_tab_holding(key, cx);
    }
}

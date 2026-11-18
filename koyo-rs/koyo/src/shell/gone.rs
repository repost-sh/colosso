//! Letting go of what was made for the checkouts, tabs and panes the workspace no longer has.

use gpui_kit::Context;

use super::{Shell, pictures};

/// What the walks for what's gone went by when each last ran: the checkouts, and the tabs and
/// panes with the terminals added.
#[derive(Default)]
pub(super) struct Keys {
    checkouts: Option<u64>,
    tabs: Option<(u64, u64)>,
}

impl Shell {
    /// Lets go of what was made for what's gone, after any change to the shell. Finding it
    /// gathers every checkout, tab and pane there is into sets, so each walk runs only once the
    /// numbers it goes by changed. A test runs them all every time and fails if one finds more.
    pub(super) fn let_go_of_what_is_gone(&mut self, cx: &mut Context<Self>) {
        let checkouts = self.workspace.checkouts_shape();
        let tabs = (self.workspace.tabs_shape(), self.terminals.added());
        let checkouts_changed = self.let_go_at.checkouts.replace(checkouts) != Some(checkouts);
        let tabs_changed = self.let_go_at.tabs.replace(tabs) != Some(tabs);
        if checkouts_changed {
            self.let_go_of_checkouts_gone(cx);
        }
        if tabs_changed {
            self.let_go_of_tabs_gone(cx);
        }
        #[cfg(test)]
        {
            let missed = |what| format!("the numbers said no {what} went, and one had");
            assert!(
                checkouts_changed || self.let_go_of_checkouts_gone(cx) == 0,
                "{}",
                missed("checkout")
            );
            assert!(
                tabs_changed || self.let_go_of_tabs_gone(cx) == 0,
                "{}",
                missed("tab or pane")
            );
        }
        pictures::retain(&self.workspace, cx);
    }

    /// Lets go of the terminal and chat views of the panes gone, and says how many things it
    /// let go of.
    fn let_go_of_tabs_gone(&mut self, cx: &mut Context<Self>) -> usize {
        self.terminals.retain(&self.workspace) + self.chat_tabs.retain(&self.workspace, cx)
    }
}

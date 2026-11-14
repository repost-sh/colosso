//! Showing the tab ⌃⇥ picked wherever it is now (`docs/tab-switcher.md` 2): as a press on its
//! row or its tile does, whichever project and space the window shows meanwhile.

use gpui_kit::{Context, Window};

use crate::model::Spot;
use crate::navigation::Route;
use crate::settings;
use crate::shell::Shell;

impl Shell {
    /// Shows the tab known by `key`, its first pane's, if it's still open. A worktree's tab
    /// shows in its project, which the shell follows into its space, and takes the keyboard,
    /// its file if it shows one; a pinned page as its tile picked shows it; a loose page in its
    /// space, as its row picked shows it.
    pub(super) fn show_visited(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        match self.workspace.spot_of(key) {
            Some(Spot::Checkout(project, branch, at)) => {
                self.workspace.select_tab(project, branch, at);
                self.navigate(Route::Workspace, cx);
                let shown = self.workspace.tab_shown();
                let file = shown.and_then(|tab| tab.files().first().copied().cloned());
                match file {
                    Some(file) => file.update(cx, |file, cx| file.focus(window, cx)),
                    None => self.keys_to_shown_tab(cx),
                }
                cx.notify();
            }
            Some(Spot::Pinned) => self.pick_pinned(key, cx),
            Some(Spot::Loose(space)) => {
                let settings = settings::get(cx);
                let index = settings.spaces.iter().position(|kept| kept.id == space);
                if let Some(index) = index.filter(|index| *index != settings.space) {
                    self.show_space(index, cx);
                }
                self.pick_loose(key, cx);
            }
            None => {}
        }
    }
}

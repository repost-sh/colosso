//! ⌘⇧T: the tab closed last comes back where it was, its pages opening again at the
//! addresses they showed.

use gpui_kit::{Context, Window};

use super::Shell;
use crate::model::PaneKind;
use crate::navigation::Route;

impl Shell {
    /// Opens the tab closed last again, on its checkout at its place, and shows it.
    pub(super) fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((closed, at)) = self.workspace.take_closed() else {
            return;
        };
        let project = self.workspace.projects[at.0].path.clone();
        let mut tab = closed.tab;
        for pane in tab.panes_mut() {
            if pane.kind != PaneKind::Browser {
                continue;
            }
            let address = (closed.addresses.iter())
                .find(|(id, _)| *id == pane.id)
                .map(|(_, address)| address.clone())
                .unwrap_or_default();
            // Titled as it was until the page says otherwise.
            pane.browser = Some(self.browser_view(address, &project, window, cx));
        }
        self.workspace.restore_tab(at, tab);
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }
}

//! What the window shows, kept while an operation makes or opens something the person isn't
//! switched to (`select` false, or left out with the person's setting off): the page (the
//! workspace or a settings page), the selected checkout, the tab the checkout opened on
//! shows, and what has the keyboard. Opening goes through the selected checkout, as the
//! person's menus do, so it's selected for the opening and put back right after, in the same
//! change: the window never draws what it passed through.

use gpui_kit::{App, Context, FocusHandle, Window};

use crate::navigation::Route;
use crate::shell::Shell;

/// Where the window was before an opening.
pub(crate) struct Kept {
    route: Route,
    /// The selected project and its selected checkout.
    selected: (usize, usize),
    /// The checkout opened on, and a pane of the tab it showed, when it had one: found by
    /// the pane, since an opening may take a tab out before it.
    target: (usize, usize, Option<u64>),
    /// What had the keyboard, which a new page's address or a file's editor would take.
    focused: Option<FocusHandle>,
}

impl Kept {
    /// The window as it is, an opening on the checkout at `(project, branch)` to come.
    pub(crate) fn of(
        shell: &Shell,
        (project, branch): (usize, usize),
        window: &Window,
        cx: &App,
    ) -> Kept {
        let workspace = &shell.workspace;
        let unit = &workspace.projects[project].branches[branch];
        Kept {
            route: shell.route(cx),
            selected: (
                workspace.active_project,
                workspace
                    .project()
                    .map_or(0, |project| project.active_branch),
            ),
            target: (
                project,
                branch,
                unit.tabs.get(unit.active_tab).map(|tab| tab.first().id),
            ),
            focused: window.focused(cx),
        }
    }

    /// Puts the window back as it was: the tab the checkout opened on showed, the checkout
    /// selected, the settings page left for the opening, and the keyboard where it was.
    pub(crate) fn restore(&self, shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) {
        let (project, branch, pane) = self.target;
        if let Some(unit) = shell.workspace.unit_mut(project, branch)
            && let Some(at) = pane.and_then(|pane| unit.tab_with(pane))
        {
            unit.active_tab = at;
        }
        let (project, branch) = self.selected;
        if shell.workspace.unit_mut(project, branch).is_some() {
            shell.workspace.select_branch(project, branch);
        } else if project < shell.workspace.projects.len() {
            shell.workspace.active_project = project;
        }
        shell.follow_selection(window, cx);
        shell.come_back(self.route, cx);
        match &self.focused {
            Some(focused) if !focused.is_focused(window) => window.focus(focused, cx),
            Some(_) => {}
            None => window.blur(cx),
        }
        cx.notify();
    }
}

//! Where the launcher opened: the checkout a tab would open on, the tabs open on it, which a row
//! switches to rather than opening again, and its chats no tab shows, which a row brings back.
//! Taken as it opens, as nothing changes behind it while it's up.

use std::sync::Arc;

use gpui_kit::{App, Image, SharedString};
use koyo_agent::protocol::ids::ThreadId;

use crate::history::{self, Place};
use crate::model::PaneKind;
use crate::shell::Shell;

pub struct Here {
    pub place: Place,
    /// The checkout's panes, but those of the tab shown, which there's no switching to.
    pub tabs: Vec<OpenTab>,
    /// The checkout's chats no tab shows, the latest first.
    pub chats: Vec<(ThreadId, SharedString)>,
}

pub struct OpenTab {
    pub pane: u64,
    pub kind: PaneKind,
    pub title: SharedString,
    /// A browser pane's address; empty for the rest.
    pub url: String,
    pub favicon: Option<Arc<Image>>,
}

impl Here {
    /// Where `shell` would open a tab now, if a checkout is selected.
    pub fn of(shell: &Shell, cx: &App) -> Option<Here> {
        let (project, unit) = (shell.workspace.project()?, shell.workspace.branch()?);
        let place = Place {
            worktree: unit.worktree.path.clone(),
            project: project.path.clone(),
        };
        let shown: Vec<u64> = (unit.tabs.get(unit.active_tab))
            .map(|tab| tab.panes().iter().map(|pane| pane.id).collect())
            .unwrap_or_default();
        let tabs = (unit.tabs.iter())
            .flat_map(|tab| tab.panes())
            .filter(|pane| !shown.contains(&pane.id))
            .filter_map(|pane| {
                let (url, favicon) = match &pane.browser {
                    Some(view) if view.read(cx).is_inspector() => return None,
                    Some(view) => (view.read(cx).address(), view.read(cx).favicon()),
                    None => (String::new(), None),
                };
                let title = pane.title.clone();
                Some(OpenTab { pane: pane.id, kind: pane.kind, title, url, favicon })
            })
            .collect();
        let chats = shell.chats_without_tabs(&unit.worktree.path, cx);
        Some(Here { place, tabs, chats })
    }

    /// The open tabs of `kind`.
    pub fn tabs_of(&self, kind: PaneKind) -> impl Iterator<Item = &OpenTab> {
        self.tabs.iter().filter(move |tab| tab.kind == kind)
    }

    /// The browser tab showing the page at `url`, as Firefox matches them: whatever its scheme,
    /// `www.` or trailing slash.
    pub fn tab_showing(&self, url: &str) -> Option<&OpenTab> {
        let key = history::same_page_key(url);
        self.tabs_of(PaneKind::Browser).find(|tab| history::same_page_key(&tab.url) == key)
    }
}

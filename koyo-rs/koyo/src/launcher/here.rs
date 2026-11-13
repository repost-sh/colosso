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
    /// Whether only a page opens here: no checkout is selected for a chat, a terminal or a
    /// worktree to open on, as in a space with no project (`docs/arc-sidebar.md` 11).
    pub pages_only: bool,
}

pub struct OpenTab {
    pub pane: u64,
    pub kind: PaneKind,
    pub title: SharedString,
    /// A browser pane's address; empty for the rest.
    pub url: String,
    /// A device pane's device, by its id.
    pub device: Option<String>,
    pub favicon: Option<Arc<Image>>,
}

impl Here {
    /// Where `shell` would open a tab now. With no checkout selected, as in a space with no
    /// project, the Arc layout still opens a page, a loose one of the space, and has a pinned or
    /// a loose page's address to change: the place is no project's. The Orca layout, which
    /// lists no such pages, opens nothing then.
    pub fn of(shell: &Shell, cx: &App) -> Option<Here> {
        let workspace = &shell.workspace;
        let (Some(project), Some(unit)) = (workspace.project(), workspace.branch()) else {
            if crate::settings::get(cx).layout != crate::settings::Layout::Arc {
                return None;
            }
            let nowhere = std::path::PathBuf::from(crate::browser::NO_PROJECT);
            let place = Place {
                worktree: nowhere.clone(),
                project: nowhere,
            };
            return Some(Here {
                place,
                tabs: Vec::new(),
                chats: Vec::new(),
                pages_only: true,
            });
        };
        let place = Place {
            worktree: unit.worktree.path.clone(),
            project: project.path.clone(),
        };
        // A pinned or a loose page shown is no tab of the checkout's: all of its tabs can be
        // switched to.
        let shown: Vec<u64> = (workspace.shown_checkout_tab())
            .and_then(|at| workspace.tab(at))
            .map(|tab| tab.panes().iter().map(|pane| pane.id).collect())
            .unwrap_or_default();
        let tabs = (unit.tabs.iter())
            .flat_map(|tab| tab.panes())
            .filter(|pane| !shown.contains(&pane.id))
            .filter_map(|pane| {
                if pane.is_inspector(cx) {
                    return None;
                }
                // A page brought back at launch and not shown yet has the address it opens at.
                let (url, favicon) = (pane.address(cx).unwrap_or_default(), pane.favicon(cx));
                let title = pane.title.clone();
                let device = (pane.device_view())
                    .and_then(|view| Some(view.read(cx).chosen()?.udid.clone()));
                Some(OpenTab {
                    pane: pane.id,
                    kind: pane.kind,
                    title,
                    url,
                    device,
                    favicon,
                })
            })
            .collect();
        let chats = shell.chats_without_tabs(&unit.worktree.path, cx);
        Some(Here {
            place,
            tabs,
            chats,
            pages_only: false,
        })
    }

    /// The open tabs of `kind`.
    pub fn tabs_of(&self, kind: PaneKind) -> impl Iterator<Item = &OpenTab> {
        self.tabs.iter().filter(move |tab| tab.kind == kind)
    }

    /// The browser tab showing the page at `url`, as Firefox matches them: whatever its scheme,
    /// `www.` or trailing slash.
    pub fn tab_showing(&self, url: &str) -> Option<&OpenTab> {
        let key = history::same_page_key(url);
        self.tabs_of(PaneKind::Browser)
            .find(|tab| history::same_page_key(&tab.url) == key)
    }
}

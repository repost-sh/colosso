//! Where the launcher opened: the checkout a tab would open on, the tabs open on it, which a row
//! switches to rather than opening again, and its chats no tab shows, which a row brings back;
//! or, for an unpinned page, no project's place. Taken as it opens, as nothing changes behind it
//! while it's up.

use std::sync::Arc;

use gpui_kit::{App, Image, SharedString};
use koyo_agent::protocol::ids::ThreadId;

use super::Opens;
use crate::history::{self, Place};
use crate::model::{Front, PaneKind};
use crate::shell::Shell;

pub struct Here {
    pub place: Place,
    /// The checkout's panes, but those of the tab shown, which there's no switching to.
    pub tabs: Vec<OpenTab>,
    /// The checkout's chats no tab shows, the latest first.
    pub chats: Vec<(ThreadId, SharedString)>,
    /// What opens here: any kind of tab on the branch the person is in, or only an unpinned page
    /// (`docs/layout.md`).
    pub opens: Opens,
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
    /// Where `shell` opens what's `asked` for now. A tab opens on the branch the person is in.
    /// Asked for an unpinned page, or in no branch, as with a pinned or an unpinned page shown or
    /// no branch selected, as in a space with no project, only a page opens, unpinned in the
    /// space shown, and the place is no project's; a pinned or an unpinned page's address is
    /// changed there too.
    pub fn of(shell: &Shell, asked: Opens, cx: &App) -> Here {
        let workspace = &shell.workspace;
        let in_branch = (workspace.project().zip(workspace.branch()))
            .filter(|_| !asked.unpinned() && !shell.opens_loose());
        let Some((project, unit)) = in_branch else {
            return Here::unpinned();
        };
        let place = Place {
            worktree: unit.worktree.path.clone(),
            project: project.path.clone(),
        };
        let shown: Vec<u64> = match workspace.front() {
            Some(Front::Checkout(at)) => workspace.tab(at),
            _ => None,
        }
        .map(|tab| tab.panes().iter().map(|pane| pane.id).collect())
        .unwrap_or_default();
        let tabs = (unit.tabs.iter())
            .flat_map(|tab| tab.panes())
            .filter(|pane| !shown.contains(&pane.id))
            .filter_map(|pane| {
                if pane.is_inspector() {
                    return None;
                }
                let (url, favicon) = (pane.address().unwrap_or_default().to_owned(), pane.favicon());
                let title = pane.title.clone();
                let device = (pane.device_view())
                    .and_then(|view| Some(view.read(cx).chosen()?.udid.clone()));
                Some(OpenTab {
                    pane: pane.id,
                    kind: pane.kind(),
                    title,
                    url,
                    device,
                    favicon,
                })
            })
            .collect();
        let chats = shell.chats_without_tabs(&unit.worktree.path, cx);
        Here {
            place,
            tabs,
            chats,
            opens: Opens::Tab,
        }
    }

    /// Where an unpinned page opens: no project's place, with no branch's tab or chat to go to.
    fn unpinned() -> Here {
        let nowhere = std::path::PathBuf::from(crate::browser::NO_PROJECT);
        let place = Place {
            worktree: nowhere.clone(),
            project: nowhere,
        };
        Here {
            place,
            tabs: Vec::new(),
            chats: Vec::new(),
            opens: Opens::UnpinnedPage,
        }
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

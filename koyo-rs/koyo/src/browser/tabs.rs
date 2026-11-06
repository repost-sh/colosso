//! Browser tabs kept for the next launch (`docs/browser.md`): each checkout's page tabs, in
//! order, with their addresses and titles, in a file in the app's data folder. What's kept
//! follows from what the panes show, after any change to them, whichever way a tab came or
//! went; it's written a moment later and on the way out. A checkout's tabs come back once it's
//! listed, without taking the focus, and none of their pages opens until its tab is shown.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{App, Context, Window};
use serde::{Deserialize, Serialize};

use super::toolbar::tab_title;
use crate::model::{Pane, Tab, Unloaded, Workspace};
use crate::shell::Shell;
use crate::{atomic_file, history, settings};

/// What the file is called, in the app's data folder.
const FILE: &str = "browser-tabs.json";
/// How long after a change the tabs are written, with whatever else changed meanwhile.
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// A checkout's page tabs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptCheckout {
    /// The main checkout of the project it's in, so its tabs go when the project does.
    pub project: PathBuf,
    pub checkout: PathBuf,
    /// In the order the tabs had, left to right.
    pub tabs: Vec<KeptTab>,
}

/// A page tab, as it comes back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Serialize, Deserialize)]
struct File {
    #[serde(default)]
    checkouts: Vec<KeptCheckout>,
}

pub struct KeptTabs {
    /// Where they're kept, if the app has a folder to keep them in.
    path: Option<PathBuf>,
    /// The checkouts read from the file whose tabs aren't back yet: carried through every
    /// write until their checkout is listed, or their project is gone or listed without it.
    waiting: Vec<KeptCheckout>,
    /// What the file holds, or will once the write due is done.
    kept: Vec<KeptCheckout>,
    /// Whether a write is due, which a change made meanwhile joins.
    saving: bool,
}

impl KeptTabs {
    /// What the file in the app's data folder holds; tests keep none.
    pub fn new(cx: &mut Context<Shell>) -> Self {
        let path = settings::data_dir()
            .filter(|_| !cfg!(test))
            .map(|data| data.join(FILE));
        // A change still settling when the app quits or the window closes is written then, as
        // the next window reads the file again.
        cx.on_app_quit(|shell, _| {
            shell.kept_tabs.flush();
            async {}
        })
        .detach();
        cx.on_release(|shell, _| shell.kept_tabs.flush()).detach();
        Self::at(path)
    }

    /// What the file at `path` holds.
    fn at(path: Option<PathBuf>) -> Self {
        let kept = path.as_deref().map(read_file).unwrap_or_default();
        KeptTabs {
            path,
            waiting: kept.clone(),
            kept,
            saving: false,
        }
    }

    /// Where to write what's kept, and its JSON, if a write is due, which then no longer is.
    fn take_due(&mut self) -> Option<(PathBuf, Vec<u8>)> {
        if !std::mem::take(&mut self.saving) {
            return None;
        }
        Some((self.path.clone()?, encode(&self.kept)?))
    }

    /// Writes what's kept now if a write is due.
    fn flush(&mut self) {
        if let Some((path, json)) = self.take_due() {
            atomic_file::write(&path, &json);
        }
    }

    /// Drops what waits for a checkout that won't come: its project gone, or listed without it.
    fn retain_waiting(&mut self, workspace: &Workspace) {
        self.waiting.retain(|waiting| {
            let project =
                (workspace.projects.iter()).find(|project| project.path == waiting.project);
            project.is_some_and(|project| {
                project.branches.is_empty()
                    || (project.branches.iter()).any(|unit| unit.worktree.path == waiting.checkout)
            })
        });
    }

    /// What's kept: each listed checkout's tabs, as the panes show them, then the tabs still
    /// waiting for their checkout.
    fn now(&self, workspace: &Workspace, cx: &App) -> Vec<KeptCheckout> {
        let mut now = Vec::new();
        for project in &workspace.projects {
            for unit in &project.branches {
                let tabs: Vec<KeptTab> = (unit.tabs.iter())
                    .flat_map(Tab::panes)
                    .filter_map(|pane| kept_tab(pane, cx))
                    .collect();
                if !tabs.is_empty() {
                    now.push(KeptCheckout {
                        project: project.path.clone(),
                        checkout: unit.worktree.path.clone(),
                        tabs,
                    });
                }
            }
        }
        now.extend(self.waiting.iter().cloned());
        now
    }
}

/// A page pane as it's kept: its address and title. Not `DevTools`, nor a tab with no
/// address yet, which leaves nothing behind.
fn kept_tab(pane: &Pane, cx: &App) -> Option<KeptTab> {
    if pane.is_inspector(cx) {
        return None;
    }
    let url = pane.address(cx).filter(|url| !url.is_empty())?;
    Some(KeptTab {
        url,
        title: pane.title.to_string(),
    })
}

/// What's in the file, or nothing if it can't be read: tabs aren't worth stopping for.
fn read_file(path: &Path) -> Vec<KeptCheckout> {
    let read = std::fs::read(path).and_then(|json| {
        let file: File = serde_json::from_slice(&json)?;
        Ok(file.checkouts)
    });
    read.unwrap_or_else(|error: io::Error| {
        if error.kind() != io::ErrorKind::NotFound {
            log::warn!("couldn't read {}: {error}", path.display());
        }
        Vec::new()
    })
}

fn encode(kept: &[KeptCheckout]) -> Option<Vec<u8>> {
    let file = File {
        checkouts: kept.to_vec(),
    };
    serde_json::to_vec_pretty(&file)
        .inspect_err(|error| log::error!("couldn't encode the browser tabs: {error}"))
        .ok()
}

impl Shell {
    /// Keeps what the panes show now, after any change to them: written a moment later when
    /// it's not what's kept already.
    pub(crate) fn keep_browser_tabs(&mut self, cx: &mut Context<Self>) {
        self.kept_tabs.retain_waiting(&self.workspace);
        let now = self.kept_tabs.now(&self.workspace, cx);
        if now == self.kept_tabs.kept {
            return;
        }
        self.kept_tabs.kept = now;
        let kept = &mut self.kept_tabs;
        // A write already due takes this change along.
        if kept.path.is_none() || std::mem::replace(&mut kept.saving, true) {
            return;
        }
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            // Written on the way out meanwhile, it's no longer due.
            let due = shell.update(cx, |shell, _| shell.kept_tabs.take_due());
            if let Ok(Some((path, json))) = due {
                cx.background_executor()
                    .spawn(async move { atomic_file::write(&path, &json) })
                    .await;
            }
        })
        .detach();
    }

    /// Brings back the tabs of the checkouts listed now, once a window: after the tabs there,
    /// in the order they had, the tab shown staying shown. Each is titled as it was, with its
    /// site's icon if one is kept, and has no page until it's shown.
    pub(crate) fn restore_browser_tabs(&mut self, cx: &mut Context<Self>) {
        let waiting = std::mem::take(&mut self.kept_tabs.waiting);
        let (listed, waiting): (Vec<_>, Vec<_>) = (waiting.into_iter())
            .partition(|kept| self.workspace.project_of(&kept.checkout).is_some());
        self.kept_tabs.waiting = waiting;
        for kept in listed {
            for tab in kept.tabs {
                let title = tab_title(&tab.title, &tab.url).into();
                let unloaded = Unloaded {
                    favicon: history::icon(&tab.url, cx),
                    address: tab.url,
                };
                self.workspace
                    .add_unloaded_tab(&kept.checkout, unloaded, title);
            }
        }
    }

    /// Makes the pages the shown tab's browser panes don't have yet, at the addresses they
    /// kept, so the panes draw them. Each opens its page once laid out; its first load is no
    /// new visit, as it was visited before.
    pub fn make_shown_browsers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let missing: Vec<(u64, String)> = (self.workspace.branch_mut())
            .and_then(|unit| unit.tabs.get_mut(unit.active_tab))
            .map(|tab| {
                (tab.panes_mut().into_iter())
                    .filter_map(|pane| Some((pane.id, pane.unloaded.take()?.address)))
                    .collect()
            })
            .unwrap_or_default();
        if missing.is_empty() {
            return;
        }
        let Some(project) = (self.workspace.project()).map(|project| project.path.clone()) else {
            return;
        };
        for (id, address) in missing {
            let view = self.browser_view(address, &project, window, cx);
            view.update(cx, |view, _| view.mark_restored());
            if let Some(pane) = (self.workspace.branch_mut()).and_then(|unit| unit.pane_mut(id)) {
                pane.browser = Some(view);
            }
        }
    }

    /// From now on the tabs are kept at `path`, as if the app had just started.
    #[cfg(test)]
    pub(crate) fn keep_browser_tabs_at(&mut self, path: &Path) {
        self.kept_tabs = KeptTabs::at(Some(path.to_owned()));
    }
}

#[cfg(test)]
mod tests;

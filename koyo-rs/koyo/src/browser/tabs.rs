//! Browser tabs kept for the next launch (`docs/browser.md`): each checkout's page tabs, in
//! order, with their addresses and titles, in a file in the app's data folder. What's kept
//! follows from what the panes show, after any change to them, whichever way a tab came or
//! went; it's written a moment later and on the way out. A checkout's tabs come back once it's
//! listed, without taking the focus, and none of their pages opens until its tab is shown.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{App, Context, Subscription, Task, Window};
use serde::{Deserialize, Serialize};

use super::toolbar::tab_title;
use crate::model::{Pane, PaneKind, Unloaded, Workspace};
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

#[derive(Default, Serialize, Deserialize)]
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
    /// What the file holds, or will once the write coming is done.
    kept: Vec<KeptCheckout>,
    /// The write coming, which a change made meanwhile joins.
    saving: Option<Task<()>>,
    _quit: Subscription,
}

impl KeptTabs {
    /// What the file in the app's data folder holds; tests keep none.
    pub fn new(cx: &mut Context<Shell>) -> Self {
        let path = settings::data_dir()
            .filter(|_| !cfg!(test))
            .map(|data| data.join(FILE));
        // A change still settling when the window closes is written, as the next window reads
        // the file again.
        cx.on_release(|shell, _| shell.kept_tabs.flush()).detach();
        Self::at(path, cx)
    }

    /// What the file at `path` holds. A change still settling when the app quits is written
    /// on the way out.
    fn at(path: Option<PathBuf>, cx: &mut Context<Shell>) -> Self {
        let kept = path.as_deref().map(read_file).unwrap_or_default();
        let quit = cx.on_app_quit(|shell, _| {
            shell.kept_tabs.flush();
            async {}
        });
        KeptTabs {
            path,
            waiting: kept.clone(),
            kept,
            saving: None,
            _quit: quit,
        }
    }

    /// Writes what's kept now if a write is coming.
    fn flush(&mut self) {
        if self.saving.take().is_some()
            && let Some(path) = &self.path
            && let Some(json) = encode(&self.kept)
        {
            atomic_file::write(path, &json);
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

    /// What's kept, from what the panes show and what still waits for its checkout: each
    /// listed checkout's tabs, then the tabs waiting for theirs.
    fn now(&self, workspace: &Workspace, cx: &App) -> Vec<KeptCheckout> {
        let waiting_for = |checkout: &Path| -> Vec<KeptTab> {
            (self.waiting.iter())
                .filter(|waiting| waiting.checkout == checkout)
                .flat_map(|waiting| waiting.tabs.iter().cloned())
                .collect()
        };
        let mut now = Vec::new();
        for project in &workspace.projects {
            for unit in &project.branches {
                let mut tabs: Vec<KeptTab> = (unit.tabs.iter())
                    .flat_map(|tab| tab.panes())
                    .filter_map(|pane| kept_tab(pane, cx))
                    .collect();
                tabs.extend(waiting_for(&unit.worktree.path));
                if !tabs.is_empty() {
                    now.push(KeptCheckout {
                        project: project.path.clone(),
                        checkout: unit.worktree.path.clone(),
                        tabs,
                    });
                }
            }
        }
        let listed = |checkout: &Path| now.iter().any(|kept| kept.checkout == checkout);
        let unlisted: Vec<KeptCheckout> = (self.waiting.iter())
            .filter(|waiting| !listed(&waiting.checkout))
            .cloned()
            .collect();
        now.extend(unlisted);
        now
    }
}

/// A page pane as it's kept: its address and title. Not `DevTools`, nor a tab with no
/// address yet, which leaves nothing behind.
fn kept_tab(pane: &Pane, cx: &App) -> Option<KeptTab> {
    if pane.kind != PaneKind::Browser
        || (pane.browser.as_ref()).is_some_and(|view| view.read(cx).is_inspector())
    {
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
        if kept.path.is_none() || kept.saving.is_some() {
            return;
        }
        kept.saving = Some(cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let encoded = shell.update(cx, |shell, _| {
                let kept = &mut shell.kept_tabs;
                // Dropped, the handle would cancel this very write; a change from now on is
                // the next one's.
                if let Some(save) = kept.saving.take() {
                    save.detach();
                }
                Some((kept.path.clone()?, encode(&kept.kept)?))
            });
            if let Ok(Some((path, json))) = encoded {
                cx.background_executor()
                    .spawn(async move { atomic_file::write(&path, &json) })
                    .await;
            }
        }));
    }

    /// Brings back the tabs of the checkouts listed now, once a window: after the tabs there, in the order they had,
    /// the tab shown staying shown. Each is titled as it was, with its site's icon if one is
    /// kept, and has no page until it's shown.
    pub(crate) fn restore_browser_tabs(&mut self, cx: &mut Context<Self>) {
        let waiting = std::mem::take(&mut self.kept_tabs.waiting);
        for kept in waiting {
            let listed = (self.workspace.projects.iter())
                .flat_map(|project| &project.branches)
                .any(|unit| unit.worktree.path == kept.checkout);
            if !listed {
                self.kept_tabs.waiting.push(kept);
                continue;
            }
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
        let Some(at) = self.workspace.shown_tab() else {
            return;
        };
        let Some(tab) = self.workspace.tab(at) else {
            return;
        };
        let missing: Vec<(u64, String)> = (tab.panes().into_iter())
            .filter(|pane| pane.browser.is_none())
            .filter_map(|pane| Some((pane.id, pane.unloaded.as_ref()?.address.clone())))
            .collect();
        if missing.is_empty() {
            return;
        }
        let project = self.workspace.projects[at.0].path.clone();
        for (id, address) in missing {
            let view = self.browser_view(address, &project, window, cx);
            view.update(cx, |view, _| view.mark_restored());
            let pane = (self.workspace.tab_mut(at))
                .and_then(|tab| tab.panes_mut().into_iter().find(|pane| pane.id == id));
            if let Some(pane) = pane {
                pane.browser = Some(view);
                pane.unloaded = None;
            }
        }
    }

    /// From now on the tabs are kept at `path`, as if the app had just started.
    #[cfg(test)]
    pub(crate) fn keep_browser_tabs_at(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.kept_tabs = KeptTabs::at(Some(path.to_owned()), cx);
    }
}

#[cfg(test)]
mod tests;

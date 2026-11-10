//! The window as it was, kept for the next one (`docs/browser.md`): each checkout's tabs in
//! their order, split as they were, with the tab shown, and the checkout selected in each
//! project, in a file in the app's data folder. What's kept follows from what the panes show:
//! any change to the shell, whichever way a tab came, moved or went, has it written a moment
//! later, and on the way out, as the panes show it then. A window closed and opened again from
//! the Dock reads it as a launch does.
//!
//! A checkout's tabs come back once git lists it, before the terminals the daemon keeps and the
//! chats the host does are looked for: those find their panes where they were, and bring back
//! in a tab of its own only what the window didn't know of (`terminals/restore.rs`,
//! `chat/tabs.rs`). Nothing of a tab is made until it's shown.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{App, Context};
use serde::{Deserialize, Serialize};

use super::Shell;
use crate::kept_file::{self, KeptFile};
use crate::model::{Tab, Workspace};
use crate::settings;

mod before;
mod shape;
#[cfg(test)]
mod tests;

pub use shape::{KeptPane, KeptTree};

/// What the file is called, in the app's data folder.
const FILE: &str = "window.json";
/// How long after a change it's written, with whatever else changed meanwhile.
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// A checkout's tabs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptCheckout {
    /// The main checkout of the project it's in, so its tabs go when the project does.
    pub project: PathBuf,
    pub checkout: PathBuf,
    /// The tab shown, by its place among `tabs`.
    #[serde(default)]
    pub shown: usize,
    /// In the order the tabs had, each a pane or a split of them.
    pub tabs: Vec<KeptTree>,
}

/// The checkout selected in a project, by its main checkout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptProject {
    pub project: PathBuf,
    pub checkout: PathBuf,
}

/// What the file holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    #[serde(default)]
    pub projects: Vec<KeptProject>,
    #[serde(default)]
    pub checkouts: Vec<KeptCheckout>,
}

pub struct KeptWindow {
    /// Where it's kept, if the app has a folder to keep it in.
    file: KeptFile,
    /// The checkouts read from the file whose tabs aren't back yet: carried through every
    /// write until their checkout is listed, or their project is gone or listed without it.
    waiting: Vec<KeptCheckout>,
    /// The projects read from the file whose checkout isn't selected again yet: until git
    /// lists their worktrees.
    selecting: Vec<KeptProject>,
    /// What the file holds, or will once the write due is done.
    kept: Kept,
}

/// Whether the window is kept in a file. Tests keep none; nor does a visual check, which draws
/// tabs of its own making: kept, they'd come back on its next run, and one run on the app's own
/// data folder would have the person's window come back as the check's.
fn keeps_a_file() -> bool {
    #[cfg(feature = "visual-check")]
    let checking = crate::visual::checking();
    #[cfg(not(feature = "visual-check"))]
    let checking = false;
    !cfg!(test) && !checking
}

impl KeptWindow {
    /// What the file in the app's data folder holds; tests and visual checks keep none.
    pub fn new(cx: &mut Context<Shell>) -> Self {
        let path = settings::data_dir()
            .filter(|_| keeps_a_file())
            .map(|data| data.join(FILE));
        // A change still settling when the app quits or the window closes is written then, as
        // the next window reads the file again.
        cx.on_app_quit(|shell, cx| {
            kept_file::write_now(shell.window_due(cx));
            async {}
        })
        .detach();
        cx.on_release(|shell, cx| kept_file::write_now(shell.window_due(cx)))
            .detach();
        Self::at(path)
    }

    /// What the file at `path` holds. With no such file yet, the browser tabs the build before
    /// kept beside it (`browser-tabs.json`) come back, each a tab of its own as they did then.
    pub(super) fn at(path: Option<PathBuf>) -> Self {
        let kept = path.as_deref().map(read_file).unwrap_or_default();
        KeptWindow {
            file: KeptFile::new(path),
            waiting: kept.checkouts.clone(),
            selecting: kept.projects.clone(),
            kept,
        }
    }

    /// Drops what waits for a checkout that won't come: its project gone, or listed without it.
    fn retain_waiting(&mut self, workspace: &Workspace) {
        let project = |path: &Path| workspace.projects.iter().find(|held| held.path == path);
        self.waiting.retain(|waiting| {
            project(&waiting.project).is_some_and(|project| {
                project.branches.is_empty()
                    || (project.branches.iter()).any(|unit| unit.worktree.path == waiting.checkout)
            })
        });
        (self.selecting).retain(|selecting| project(&selecting.project).is_some());
    }
}

/// What's in the file, or nothing if it can't be read: tabs aren't worth stopping for. The
/// first time there's none, what the browser tabs' own file held.
fn read_file(path: &Path) -> Kept {
    if !path.exists() {
        return Kept {
            projects: Vec::new(),
            checkouts: before::tabs_kept_before(path),
        };
    }
    kept_file::read(path).unwrap_or_default()
}

/// Of `items`, what `keep` keeps of each, in order; and among those the place of the one at
/// `shown`, or of the last kept before it: the tab shown, or the one before it when it isn't
/// kept.
fn kept_and_shown<T, U>(
    items: &[T],
    shown: usize,
    mut keep: impl FnMut(&T) -> Option<U>,
) -> (Vec<U>, usize) {
    let (mut kept, mut at) = (Vec::new(), 0);
    for (place, item) in items.iter().enumerate() {
        if let Some(item) = keep(item) {
            if place <= shown {
                at = kept.len();
            }
            kept.push(item);
        }
    }
    (kept, at)
}

impl Shell {
    /// Has the window written a moment from now, after any change to the shell: as it is
    /// then, so a run of changes, a divider dragged or a row fading, is looked at once.
    pub(crate) fn keep_window(&mut self, cx: &mut Context<Self>) {
        if self.kept.file.changed() {
            let shell = cx.weak_entity();
            kept_file::write_later(cx, SAVE_DELAY, move |cx| {
                shell.update(cx, |shell, cx| shell.window_due(cx)).ok()?
            });
        }
    }

    /// Where to write the window as it is now, and its JSON, if a write is due, which then no
    /// longer is, and the window isn't what the file holds already.
    fn window_due(&mut self, cx: &App) -> Option<(PathBuf, Vec<u8>)> {
        let path = self.kept.file.take_due()?;
        self.kept.retain_waiting(&self.workspace);
        let now = self.window_now(cx);
        if now == self.kept.kept {
            return None;
        }
        self.kept.kept = now;
        Some((path, kept_file::encode(&self.kept.kept)?))
    }

    /// What's kept: each listed project's selected checkout and each listed checkout's tabs,
    /// as the panes show them, then what still waits for its checkout or its project's list.
    fn window_now(&self, cx: &App) -> Kept {
        let mut now = Kept::default();
        for project in &self.workspace.projects {
            if let Some(unit) = project.branch() {
                now.projects.push(KeptProject {
                    project: project.path.clone(),
                    checkout: unit.worktree.path.clone(),
                });
            } else {
                // Not listed yet: what the file says of it stands.
                let waiting = self.kept.selecting.iter();
                now.projects
                    .extend(waiting.filter(|kept| kept.project == project.path).cloned());
            }
            for unit in &project.branches {
                let kept = |tab: &Tab| self.kept_tree(tab, cx);
                let (tabs, shown) = kept_and_shown(&unit.tabs, unit.active_tab, kept);
                if !tabs.is_empty() {
                    now.checkouts.push(KeptCheckout {
                        project: project.path.clone(),
                        checkout: unit.worktree.path.clone(),
                        shown,
                        tabs,
                    });
                }
            }
        }
        now.checkouts.extend(self.kept.waiting.iter().cloned());
        now
    }

    /// Brings the window back as the file has it, for the projects and checkouts git lists
    /// now, once a window: each project on the checkout it had selected, and each checkout's
    /// tabs in the order they had, split as they were, the one shown shown. Tabs opened on a
    /// checkout meanwhile come after them and stay shown. No pane's content is made until its
    /// tab is shown; a pane whose content is gone isn't brought back, and its split keeps the
    /// other side.
    pub(crate) fn restore_window(&mut self, cx: &mut Context<Self>) {
        let selecting = std::mem::take(&mut self.kept.selecting);
        for kept in selecting {
            let Some(at) = self.workspace.project_index(&kept.project) else {
                continue;
            };
            let project = &mut self.workspace.projects[at];
            if project.branches.is_empty() {
                self.kept.selecting.push(kept);
                continue;
            }
            let selected = (project.branches.iter())
                .position(|unit| unit.worktree.path == kept.checkout);
            if let Some(branch) = selected {
                project.active_branch = branch;
            }
        }
        let waiting = std::mem::take(&mut self.kept.waiting);
        let (listed, waiting): (Vec<_>, Vec<_>) = (waiting.into_iter())
            .partition(|kept| self.workspace.unit_at(&kept.checkout).is_some());
        self.kept.waiting = waiting;
        for kept in listed {
            let back = |tree: &KeptTree| self.tab_of_kept(tree, cx);
            let (tabs, shown) = kept_and_shown(&kept.tabs, kept.shown, back);
            let Some((project, branch)) = self.workspace.unit_at(&kept.checkout) else {
                continue;
            };
            let unit = &mut self.workspace.projects[project].branches[branch];
            if unit.tabs.is_empty() {
                unit.active_tab = shown;
            } else {
                unit.active_tab += tabs.len();
            }
            unit.tabs.splice(0..0, tabs);
        }
    }

    /// From now on the window is kept at `path`, as if the app had just started.
    #[cfg(test)]
    pub(crate) fn keep_window_at(&mut self, path: &Path) {
        self.kept = KeptWindow::at(Some(path.to_owned()));
    }
}

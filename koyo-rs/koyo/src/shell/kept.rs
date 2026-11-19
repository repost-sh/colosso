//! The window as it was, kept for the next one (`docs/browser.md`): each checkout's tabs in
//! their order, split as they were, with the tab shown, and the checkout selected in each
//! project, in a file in the app's data folder. What's kept follows from what the panes show:
//! any change to the shell, whichever way a tab came, moved or went, has it written a moment
//! later, and on the way out, as the panes show it then. A window closed and opened again from
//! the Dock reads it as a launch does. Each space's loose pages, which are no checkout's, are in
//! it too (`loose.rs`); the pinned pages are kept beside it in a file of their own (`pinned.rs`).
//!
//! A checkout's tabs come back once git lists it, before the terminals the daemon keeps and the
//! chats the host does are looked for: those find their panes where they were, and bring back
//! in a tab of its own only what the window didn't know of (`terminals/restore.rs`,
//! `chat/tabs.rs`). Nothing of a tab is made until it's shown.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{App, Context, Global, WeakEntity};
use serde::{Deserialize, Serialize};

use super::Shell;
use crate::blocking::{self, Blocking};
use crate::kept_file::{self, KeptFile};
use crate::model::{Tab, Workspace};
use crate::settings;

mod before;
mod loose;
mod pinned;
mod shape;
#[cfg(test)]
mod tests;

pub use loose::KeptSpace;
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
    /// Each space's loose pages, which are no checkout's (`loose.rs`); left out with none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spaces: Vec<KeptSpace>,
}

#[derive(Default)]
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
    /// The pinned pages' file, beside the window's (`pinned.rs`).
    pins: pinned::PinsFile,
}

impl KeptWindow {
    /// Where the window is kept: in the app's data folder; a test keeps it nowhere, and a
    /// test's or a visual check's window starts from [`KeptWindow::default`], which keeps none.
    pub fn path() -> Option<PathBuf> {
        settings::data_dir()
            .filter(|_| !cfg!(test))
            .map(|data| data.join(FILE))
    }

    /// What the file in the app's data folder holds.
    pub fn read(may: Blocking<'_>) -> Self {
        Self::at(Self::path(), may)
    }

    /// What the file at `path` holds. With no such file yet, the browser tabs the build before
    /// kept beside it (`browser-tabs.json`) come back, each a tab of its own as they did then.
    pub(super) fn at(path: Option<PathBuf>, may: Blocking<'_>) -> Self {
        let kept = (path.as_deref())
            .map(|path| read_file(path, may))
            .unwrap_or_default();
        let pins = (path.as_deref()).map(|path| path.with_file_name(pinned::FILE));
        KeptWindow {
            file: KeptFile::new(path),
            waiting: kept.checkouts.clone(),
            selecting: kept.projects.clone(),
            kept,
            pins: pinned::PinsFile::at(pins, may),
        }
    }

    /// The addresses of the pages it brings back: each checkout's, each space's loose ones, and
    /// the pinned ones.
    pub fn addresses(&self) -> Vec<&str> {
        fn of_tree<'a>(tree: &'a KeptTree, addresses: &mut Vec<&'a str>) {
            match tree {
                KeptTree::Split { first, second, .. } => {
                    of_tree(first, addresses);
                    of_tree(second, addresses);
                }
                KeptTree::Pane(KeptPane::Page { url, .. }) => addresses.push(url),
                KeptTree::Pane(_) => {}
            }
        }
        let mut addresses = Vec::new();
        for tab in self.kept.checkouts.iter().flat_map(|checkout| &checkout.tabs) {
            of_tree(tab, &mut addresses);
        }
        let loose = self.kept.spaces.iter().flat_map(|space| &space.tabs);
        addresses.extend(loose.map(|tab| tab.url.as_str()));
        addresses.extend(self.pins.addresses());
        addresses
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

/// The shell made last, whose window and pinned pages are written as the app quits.
struct Quitting(WeakEntity<Shell>);

impl Global for Quitting {}

impl Quitting {
    fn shell(cx: &App) -> Option<WeakEntity<Shell>> {
        Some(cx.try_global::<Self>()?.0.clone())
    }
}

/// Told how opening the window again went.
type Opened = Box<dyn FnOnce(&gpui_kit::Result<()>, &mut App)>;

/// The window opening again, its files still being read: who's told once it's open.
#[derive(Default)]
struct Reopening(Vec<Opened>);

impl Global for Reopening {}

/// Opens the window again with what the file at `at` holds, read as a launch reads it but off
/// the main thread, once the writes due as the last window closed have landed: `open` opens it
/// when the read lands, and `opened` hears how that went. Asked again meanwhile, no second
/// window opens: that `opened` hears how the first went.
pub fn reopen(
    at: Option<PathBuf>,
    open: impl FnOnce(KeptWindow, &mut App) -> gpui_kit::Result<()> + 'static,
    opened: impl FnOnce(&gpui_kit::Result<()>, &mut App) + 'static,
    cx: &mut App,
) {
    let reopening = &mut cx.default_global::<Reopening>().0;
    reopening.push(Box::new(opened));
    if reopening.len() > 1 {
        return;
    }
    let writers = kept_file::Writers::of(cx);
    let read = blocking::spawn(cx.background_executor(), move |may| {
        writers.settle(may);
        KeptWindow::at(at, may)
    });
    cx.spawn(async move |cx| {
        let kept = read.await;
        cx.update(|cx| {
            let result = open(kept, cx);
            for opened in std::mem::take(&mut cx.global_mut::<Reopening>().0) {
                opened(&result, cx);
            }
        });
    })
    .detach();
}

/// What's in the file, or nothing if it can't be read: tabs aren't worth stopping for. The
/// first time there's none, what the browser tabs' own file held.
fn read_file(path: &Path, may: Blocking<'_>) -> Kept {
    if !path.exists() {
        return Kept {
            checkouts: before::tabs_kept_before(path, may),
            ..Kept::default()
        };
    }
    kept_file::read(path, may).unwrap_or_default()
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
        self.keep_pinned(cx);
        if self.kept.file.changed() {
            let shell = cx.weak_entity();
            kept_file::write_later(cx, SAVE_DELAY, move |cx| {
                shell.update(cx, |shell, cx| shell.window_due(cx)).ok()?
            });
        }
    }

    /// Has the window and the pinned pages written as the app quits, and as the window
    /// closes, if a write of either is due then: handed to their files' writers as it closes,
    /// written with everything else waiting as the app quits. Asked once for the app, as the
    /// first shell is made, of the shell made last, the window's: a shell gone has handed
    /// what was due as it closed.
    pub(super) fn keep_on_the_way_out(cx: &mut Context<Self>) {
        if !cx.has_global::<Quitting>() {
            kept_file::write_on_quit(cx, |cx| {
                Quitting::shell(cx)?.update(cx, |shell, cx| shell.window_due(cx)).ok().flatten()
            });
            kept_file::write_on_quit(cx, |cx| {
                Quitting::shell(cx)?.update(cx, |shell, _| shell.pinned_due()).ok().flatten()
            });
        }
        let shell = cx.weak_entity();
        cx.set_global(Quitting(shell));
        cx.on_release(|shell, cx| {
            for due in shell.what_is_due(cx) {
                kept_file::write_due(due, cx);
            }
        })
        .detach();
    }

    /// What's due to be written of the window and the pinned pages, taken now, as the window
    /// closes.
    fn what_is_due(&mut self, cx: &App) -> [Option<(PathBuf, Vec<u8>)>; 2] {
        [self.window_due(cx), self.pinned_due()]
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
        now.spaces = self.kept_spaces();
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
            let selected =
                (project.branches.iter()).position(|unit| unit.worktree.path == kept.checkout);
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
        self.kept = blocking::in_test(|may| KeptWindow::at(Some(path.to_owned()), may));
    }
}

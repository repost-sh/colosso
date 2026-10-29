//! One checkout as the app holds it while it's in the sidebar: git opened on it, its project
//! listing and watching it, its changes read again as it's written to (only what the watcher
//! says was touched; every other file keeps the diff it had), the marks the file tree tints
//! with, and the files open from it. The panels showing it observe it; none reaches another.

mod documents;

use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use colosso_git::{Change, Checkout as Git};
use colosso_project::{Feed, GitDirs, Project};
use futures::StreamExt as _;
use gpui_kit::gpui::{
    AppContext as _, AsyncApp, Context, Entity, Subscription, Task, WeakEntity, Window,
};

use documents::Documents;
pub use documents::read_at_most;
pub use documents::{Content, Document, DocumentEvent, MOST_BYTES};

use crate::changes::{FileView, Marks, Read, Stale, Touched, read, touched};
use crate::checkouts;

/// Something done to some of a checkout's changes: staging, unstaging or discarding them.
pub type GitAct = fn(&Git, &[Change]) -> colosso_git::Result<()>;

/// Why reading the changes waits, if it does; it catches up once nothing does.
#[derive(Default)]
struct Waits {
    /// The window's out of sight.
    hidden: bool,
    /// Another checkout is selected.
    unselected: bool,
}

pub struct Checkout {
    root: PathBuf,
    git: Option<Arc<Git>>,
    /// Its project once git's open, which the file tree lists and the changes follow.
    project: Option<Arc<Project>>,
    documents: Documents,
    /// What status listed last, the files left out of `files` included.
    listed: Vec<Change>,
    /// The same, as the file tree marks it.
    marks: Arc<Marks>,
    files: Vec<Arc<FileView>>,
    /// Why the checkout couldn't be read, if it couldn't.
    error: Option<String>,
    /// Why changes aren't followed as they happen, if they aren't.
    unwatched: Option<String>,
    stale: Stale,
    waits: Waits,
    reading: Option<Task<()>>,
    /// Whether a read has finished, so an empty list means no changes rather than not yet.
    read_once: bool,
    /// Whether the diffs are on screen, so worth colouring.
    coloured: bool,
    colouring: Option<Task<()>>,
    _visibility: Subscription,
    /// Takes what the checkout's project reports, which keeps the project open meanwhile.
    _following: Option<Task<()>>,
}

impl Checkout {
    /// Opens the checkout at `root`, selected, and starts reading and following its changes.
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Nothing is read while nobody can see it; what changed meanwhile is read on return.
        let _visibility = cx.observe_window_visibility(window, |this, visibility, _, cx| {
            this.waits.hidden = !visibility.is_visible();
            this.read(cx);
        });
        let path = root.clone();
        let opening = cx.spawn(async move |this, cx| {
            let opened = cx
                .background_spawn(async move {
                    let git = Git::open(&path)?;
                    let (git_dir, common_dir) = git.git_dirs();
                    let dirs = GitDirs {
                        git_dir,
                        common_dir,
                    };
                    Ok::<_, colosso_git::GitError>((git, dirs))
                })
                .await;
            this.update(cx, |this, cx| this.opened(opened, cx)).ok();
        });
        Checkout {
            root,
            git: None,
            project: None,
            documents: Documents::default(),
            listed: Vec::new(),
            marks: Arc::default(),
            files: Vec::new(),
            error: None,
            unwatched: None,
            stale: Stale::everything(),
            waits: Waits::default(),
            reading: Some(opening),
            read_once: false,
            coloured: false,
            colouring: None,
            _visibility,
            _following: None,
        }
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    /// Its project, once git's open on it.
    pub fn project(&self) -> Option<&Arc<Project>> {
        self.project.as_ref()
    }

    pub fn files(&self) -> &[Arc<FileView>] {
        &self.files
    }

    /// Each changed file's mark, and each folder's above them, as last read.
    pub fn marks(&self) -> &Arc<Marks> {
        &self.marks
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn unwatched(&self) -> Option<&str> {
        self.unwatched.as_deref()
    }

    /// The document for the file at `path`, absolute, read unless it's open already.
    pub fn document(&mut self, path: &Path, cx: &mut Context<Self>) -> Entity<Document> {
        self.documents.open(path, cx)
    }

    /// The document for the file at `path`, absolute, if a view has it open.
    pub fn open_document(&self, path: &Path) -> Option<Entity<Document>> {
        self.documents.find(path)
    }

    /// The lines added and removed across the files.
    pub fn totals(&self) -> (u32, u32) {
        self.files.iter().fold((0, 0), |(added, removed), file| {
            let (a, r) = file.diff.content.line_counts();
            (added + a, removed + r)
        })
    }

    /// Colours the diffs while they're on screen; the counts alone don't need it.
    pub fn set_coloured(&mut self, coloured: bool, cx: &mut Context<Self>) {
        if self.coloured == coloured {
            return;
        }
        self.coloured = coloured;
        if coloured {
            self.colour(cx);
        } else {
            self.colouring = None;
        }
    }

    /// Reads the changes only while the checkout is selected; what was written meanwhile is
    /// read when it is again.
    pub fn set_selected(&mut self, selected: bool, cx: &mut Context<Self>) {
        self.waits.unselected = !selected;
        self.read(cx);
    }

    /// Changes already read as `files`, coloured, for measuring the panel without a checkout.
    #[cfg(test)]
    pub fn read_as(
        files: Vec<colosso_git::FileDiff>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut checkout = Checkout::new(PathBuf::from("/nowhere"), window, cx);
        checkout.reading = None;
        checkout.stale = Stale::default();
        checkout.read_once = true;
        checkout.files = (files.into_iter())
            .map(|diff| Arc::new(FileView::new(diff)))
            .inspect(|file| file.colour())
            .collect();
        checkout
    }

    /// Runs `act`, staging, unstaging or discarding, on `changes` in the background, then reads
    /// again what it touched rather than waiting for the watcher to say.
    pub fn act(
        &mut self,
        act: GitAct,
        changes: Vec<Change>,
        cx: &mut Context<Self>,
    ) -> Task<colosso_git::Result<()>> {
        let Some(git) = self.git.clone() else {
            return Task::ready(Ok(()));
        };
        let files = (changes.iter().flat_map(Change::paths)).map(|path| Touched::File(path.into()));
        let touched: Vec<Touched> = std::iter::once(Touched::Index).chain(files).collect();
        cx.spawn(async move |this, cx| {
            let done = cx
                .background_spawn(async move { act(&git, &changes) })
                .await;
            // Even a failed command may have done part of it.
            this.update(cx, |this, cx| this.touched(touched, cx)).ok();
            done
        })
    }

    /// Whether nothing has been read yet.
    pub fn is_loading(&self) -> bool {
        !self.read_once && self.error.is_none()
    }

    /// Starts following the checkout through its project, shared with every other window
    /// showing it, once git's open.
    fn opened(&mut self, opened: colosso_git::Result<(Git, GitDirs)>, cx: &mut Context<Self>) {
        self.reading = None;
        match opened {
            Err(error) => self.failed(error.message),
            Ok((git, dirs)) => {
                let project = checkouts::project(git.root(), dirs, cx);
                log::info!("following the changes in {}", self.root.display());
                let updates = project.subscribe(Feed::Written);
                self._following =
                    Some(cx.spawn(async move |this, cx| follow(this, updates, cx).await));
                self.project = Some(project);
                self.git = Some(Arc::new(git));
            }
        }
        self.read(cx);
        cx.notify();
    }

    /// Reads the changes once rather than as they happen: the platform can't watch the
    /// checkout.
    fn set_unwatched(&mut self, why: String, cx: &mut Context<Self>) {
        if self.unwatched.is_none() {
            log::warn!(
                "can't follow the changes in {}, so they're read once: {why}",
                self.root.display()
            );
        }
        self.unwatched = Some(why);
        cx.notify();
    }

    pub(crate) fn touched(&mut self, touched: Vec<Touched>, cx: &mut Context<Self>) {
        self.documents.touched(&touched, &self.root, cx);
        self.stale.add(touched);
        self.read(cx);
    }

    /// Reads what's stale, unless a read is under way, which reads again when it's done.
    fn read(&mut self, cx: &mut Context<Self>) {
        let waits = self.waits.hidden || self.waits.unselected;
        if waits || self.reading.is_some() || self.stale.is_empty() {
            return;
        }
        let Some(git) = self.git.clone() else {
            return;
        };
        let stale = mem::take(&mut self.stale);
        let listed = mem::take(&mut self.listed);
        let files = self.files.clone();
        self.reading = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    let read = read(&git, &stale, &listed, files)?;
                    let marks = Arc::new(Marks::new(&read.listed));
                    Ok::<_, colosso_git::GitError>((read, marks))
                })
                .await;
            this.update(cx, |this, cx| this.read_done(read, cx)).ok();
        }));
    }

    /// Shows why the changes couldn't be read, logging it once rather than again each time a
    /// write is noticed.
    fn failed(&mut self, error: String) {
        if self.error.as_ref() != Some(&error) {
            log::warn!(
                "couldn't read the changes in {}: {error}",
                self.root.display()
            );
        }
        self.error = Some(error);
    }

    fn read_done(&mut self, read: colosso_git::Result<(Read, Arc<Marks>)>, cx: &mut Context<Self>) {
        self.reading = None;
        match read {
            Ok((read, marks)) => {
                self.listed = read.listed;
                self.marks = marks;
                self.files = read.files;
                self.error = None;
                self.read_once = true;
                if self.coloured {
                    self.colour(cx);
                }
            }
            Err(error) => {
                // Whatever is listed next is new.
                self.listed.clear();
                self.failed(error.message);
            }
        }
        cx.notify();
        self.read(cx);
    }

    /// Colours the files not coloured yet, on as many threads as there are, top first so what's
    /// on screen colours soonest.
    fn colour(&mut self, cx: &mut Context<Self>) {
        let uncoloured: Vec<Arc<FileView>> = (self.files.iter())
            .filter(|file| file.colours().is_none())
            .cloned()
            .collect();
        if uncoloured.is_empty() {
            return;
        }
        let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        self.colouring = Some(cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            for batch in uncoloured.chunks(threads) {
                let colouring = batch
                    .iter()
                    .cloned()
                    .map(|file| executor.spawn(async move { file.colour() }));
                futures::future::join_all(colouring).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        }));
    }
}

/// Passes on what the checkout's project reports, as it hands it over: the project gathers a
/// burst of writes into one update, and what comes while a read is under way is read once
/// it's done.
async fn follow(
    this: WeakEntity<Checkout>,
    mut updates: colosso_project::Subscription,
    cx: &mut AsyncApp,
) {
    while let Some(first) = updates.next().await {
        let mut burst = touched(&first);
        let mut unwatched = first.unwatched;
        while let Some(more) = updates.try_next() {
            burst.extend(touched(&more));
            unwatched = more.unwatched.or(unwatched);
        }
        let passed = this.update(cx, |this, cx| {
            if let Some(why) = unwatched {
                this.set_unwatched(why, cx);
            }
            this.touched(burst, cx);
        });
        if passed.is_err() {
            return;
        }
    }
}

//! A checkout's changed files, kept up to date while something writes to it. Only what the
//! watcher says was touched is read again; every other file keeps the diff it had.

use std::collections::{HashMap, HashSet};
use std::mem;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use colosso_git::{Change, Checkout, Content, FileDiff, Status, changed_words};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::gpui::{AppContext as _, AsyncApp, Context, Subscription, Task, WeakEntity, Window};

use super::highlight::{Highlights, highlight};
use super::watch::{Touched, Watched, Watcher, Watching, watch};

/// How long the watcher must be quiet before the changes are read again.
const QUIET: Duration = Duration::from_millis(100);

/// The longest a burst of writes holds back a read, so a long one still shows as it goes.
const LONGEST_WAIT: Duration = Duration::from_secs(1);

/// More touched files than this are read again as if everything was.
const MOST_TOUCHED: usize = 1_000;

/// A changed file as shown: its diff and, once worked out, its colours. The diff shows plain
/// until then, as colouring takes longer than reading.
#[derive(Debug)]
pub struct FileView {
    pub diff: FileDiff,
    colours: OnceLock<Colours>,
}

/// What colouring a file works out ahead of drawing it, so drawing a line only looks it up.
#[derive(Debug, Default)]
pub struct Colours {
    /// Its syntax, if its kind of file is known.
    pub highlights: Option<Highlights>,
    /// The words that changed in each replaced line, by side and line.
    pub old_words: HashMap<u32, Vec<Range<usize>>>,
    pub new_words: HashMap<u32, Vec<Range<usize>>>,
}

impl FileView {
    pub(super) fn new(diff: FileDiff) -> Self {
        FileView {
            diff,
            colours: OnceLock::new(),
        }
    }

    pub fn key(&self) -> Key {
        key(&self.diff.change)
    }

    pub fn colours(&self) -> Option<&Colours> {
        self.colours.get()
    }

    fn colour(&self) {
        self.colours.get_or_init(|| colours(&self.diff));
    }
}

/// `diff`'s syntax, and the words each removed line and the line put in its place changed:
/// the nth of a hunk's removed lines is paired with the nth of its added ones.
fn colours(diff: &FileDiff) -> Colours {
    let mut colours = Colours {
        highlights: highlight(&diff.change.path, &diff.content),
        ..Colours::default()
    };
    if let Content::Text { old, new, hunks } = &diff.content {
        for hunk in hunks {
            for (old_line, new_line) in hunk.old.clone().zip(hunk.new.clone()) {
                let words = changed_words(old.line(old_line as usize), new.line(new_line as usize));
                if let Some((removed, added)) = words {
                    colours.old_words.insert(old_line, removed);
                    colours.new_words.insert(new_line, added);
                }
            }
        }
    }
    colours
}

/// What has to be read again.
#[derive(Debug, Default)]
struct Stale {
    files: HashSet<String>,
    /// Whether the index changed, which is what staged changes, and the unstaged ones beside
    /// them, are diffed from.
    index: bool,
    everything: bool,
}

impl Stale {
    fn is_empty(&self) -> bool {
        !self.everything && !self.index && self.files.is_empty()
    }

    fn add(&mut self, touched: Vec<Touched>) {
        for touched in touched {
            match touched {
                Touched::File(path) => {
                    self.files.insert(path);
                }
                Touched::Index => self.index = true,
                Touched::Head => self.everything = true,
            }
        }
        if self.files.len() > MOST_TOUCHED {
            self.files.clear();
            self.everything = true;
        }
    }

    /// Whether `change` is stale; with the index changed, one of `staged`'s paths is.
    fn has(&self, change: &Change, staged: &HashSet<&str>) -> bool {
        let touched = |path: &str| self.files.contains(path);
        self.everything
            || (self.index && staged.contains(change.path.as_str()))
            // Writing a file changes only what's unstaged of it.
            || !change.staged
                && (touched(&change.path)
                    || matches!(&change.status, Status::Renamed { from } if touched(from)))
    }
}

/// Something done to some of a checkout's changes: staging, unstaging or discarding them.
pub type GitAct = fn(&Checkout, &[Change]) -> colosso_git::Result<()>;

/// One checkout's changes.
pub struct Changes {
    root: PathBuf,
    checkout: Option<Arc<Checkout>>,
    /// What status listed last, the files left out of `files` included.
    listed: Vec<Change>,
    files: Vec<Arc<FileView>>,
    /// Why the checkout couldn't be read, if it couldn't.
    error: Option<String>,
    /// Why changes aren't followed as they happen, if they aren't.
    unwatched: Option<String>,
    stale: Stale,
    /// Whether the window is out of sight, so reading waits until it's back.
    paused: bool,
    reading: Option<Task<()>>,
    /// Whether a read has finished, so an empty list means no changes rather than not yet.
    read_once: bool,
    /// Whether the diffs are on screen, so worth colouring.
    coloured: bool,
    colouring: Option<Task<()>>,
    _visibility: Subscription,
    _watcher: Option<Watcher>,
    _following: Option<Task<()>>,
}

/// What a read found, to put in place of what was shown.
struct Read {
    listed: Vec<Change>,
    files: Vec<Arc<FileView>>,
}

impl Changes {
    /// Opens the checkout at `root` and starts reading and following its changes.
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Nothing is read while nobody can see it; what changed meanwhile is read on return.
        let _visibility = cx.observe_window_visibility(window, |this, visibility, _, cx| {
            this.set_visible(visibility.is_visible(), cx);
        });
        let path = root.clone();
        let opening = cx.spawn(async move |this, cx| {
            let opened = cx
                .background_spawn(async move {
                    let checkout = Checkout::open(&path)?;
                    let (git_dir, common_dir) = checkout.git_dirs();
                    let watched = watch(Watched {
                        root: checkout.root().to_owned(),
                        git_dir,
                        common_dir,
                    });
                    Ok::<_, colosso_git::GitError>((checkout, watched))
                })
                .await;
            this.update(cx, |this, cx| this.opened(opened, cx)).ok();
        });
        Changes {
            root,
            checkout: None,
            listed: Vec::new(),
            files: Vec::new(),
            error: None,
            unwatched: None,
            stale: Stale {
                everything: true,
                ..Stale::default()
            },
            paused: false,
            reading: Some(opening),
            read_once: false,
            coloured: false,
            colouring: None,
            _visibility,
            _watcher: None,
            _following: None,
        }
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn files(&self) -> &[Arc<FileView>] {
        &self.files
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn unwatched(&self) -> Option<&str> {
        self.unwatched.as_deref()
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

    /// Changes already read as `files`, coloured, for measuring the panel without a checkout.
    #[cfg(test)]
    pub fn read_as(files: Vec<FileDiff>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut changes = Changes::new(PathBuf::from("/nowhere"), window, cx);
        changes.reading = None;
        changes.stale = Stale::default();
        changes.read_once = true;
        changes.files = (files.into_iter())
            .map(|diff| Arc::new(FileView::new(diff)))
            .inspect(|file| file.colour())
            .collect();
        changes
    }

    /// Runs `act`, staging, unstaging or discarding, on `changes` in the background, then reads
    /// again what it touched rather than waiting for the watcher to say.
    pub fn act(
        &mut self,
        act: GitAct,
        changes: Vec<Change>,
        cx: &mut Context<Self>,
    ) -> Task<colosso_git::Result<()>> {
        let Some(checkout) = self.checkout.clone() else {
            return Task::ready(Ok(()));
        };
        let files = (changes.iter().flat_map(Change::paths)).map(|path| Touched::File(path.into()));
        let touched: Vec<Touched> = std::iter::once(Touched::Index).chain(files).collect();
        cx.spawn(async move |this, cx| {
            let done = cx
                .background_spawn(async move { act(&checkout, &changes) })
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

    /// Stops reading while the window is out of sight, and catches up when it's back.
    fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.paused = !visible;
        self.read(cx);
    }

    fn opened(
        &mut self,
        opened: colosso_git::Result<(Checkout, Watching)>,
        cx: &mut Context<Self>,
    ) {
        self.reading = None;
        match opened {
            Err(error) => self.error = Some(error.message),
            Ok((checkout, watching)) => {
                self.checkout = Some(Arc::new(checkout));
                match watching {
                    Ok((watcher, touched)) => {
                        self._watcher = Some(watcher);
                        self._following =
                            Some(cx.spawn(async move |this, cx| follow(this, touched, cx).await));
                    }
                    Err(error) => self.unwatched = Some(error.to_string()),
                }
            }
        }
        self.read(cx);
        cx.notify();
    }

    fn touched(&mut self, touched: Vec<Touched>, cx: &mut Context<Self>) {
        self.stale.add(touched);
        self.read(cx);
    }

    /// Reads what's stale, unless a read is under way, which reads again when it's done.
    fn read(&mut self, cx: &mut Context<Self>) {
        if self.paused || self.reading.is_some() || self.stale.is_empty() {
            return;
        }
        let Some(checkout) = self.checkout.clone() else {
            return;
        };
        let stale = mem::take(&mut self.stale);
        let listed = mem::take(&mut self.listed);
        let files = self.files.clone();
        self.reading = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { read(&checkout, &stale, &listed, files) })
                .await;
            this.update(cx, |this, cx| this.read_done(read, cx)).ok();
        }));
    }

    fn read_done(&mut self, read: colosso_git::Result<Read>, cx: &mut Context<Self>) {
        self.reading = None;
        match read {
            Ok(read) => {
                self.listed = read.listed;
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
                self.error = Some(error.message);
            }
        }
        cx.notify();
        self.read(cx);
    }

    /// Colours the files not coloured yet, on as many threads as there are, top first so what's
    /// on screen colours soonest.
    fn colour(&mut self, cx: &mut Context<Self>) {
        let uncoloured: Vec<Arc<FileView>> = (self.files.iter())
            .filter(|file| file.colours.get().is_none())
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

/// Lists the changes again and diffs the files that are new, changed in status, or `stale`,
/// keeping the rest of `files` as they are.
fn read(
    checkout: &Checkout,
    stale: &Stale,
    listed: &[Change],
    files: Vec<Arc<FileView>>,
) -> colosso_git::Result<Read> {
    let changes = checkout.changes()?;
    let unread = unread(&changes, listed, stale);
    let reread: HashSet<Key> = unread.iter().map(key).collect();
    let diffs = checkout.diffs(&unread);
    let read = diffs
        .into_iter()
        .map(|diff| Arc::new(FileView::new(diff)))
        .collect();
    Ok(Read {
        files: merged(&changes, files, &reread, read),
        listed: changes,
    })
}

/// Which of `changes` to read again: the `stale` ones and any `listed` differently before.
fn unread(changes: &[Change], listed: &[Change], stale: &Stale) -> Vec<Change> {
    // Staging a file moves what its unstaged change is diffed from, so with the index changed,
    // every file staged before or now is read again.
    let staged: HashSet<&str> = (listed.iter().chain(changes))
        .filter(|change| change.staged)
        .map(|change| change.path.as_str())
        .collect();
    let listed: HashSet<&Change> = listed.iter().collect();
    changes
        .iter()
        .filter(|change| stale.has(change, &staged) || !listed.contains(change))
        .cloned()
        .collect()
}

/// What tells a file's changes apart: whether it's staged, and its path. A file partly staged
/// has a change on each side.
pub type Key = (bool, String);

pub fn key(change: &Change) -> Key {
    (change.staged, change.path.clone())
}

/// The files for `changes`, in their order: those `read`, and the rest of `shown` as they were.
/// A file `reread` that isn't in `read` has no change left on its side, so has no diff.
fn merged(
    changes: &[Change],
    shown: Vec<Arc<FileView>>,
    reread: &HashSet<Key>,
    read: Vec<Arc<FileView>>,
) -> Vec<Arc<FileView>> {
    let mut files: HashMap<Key, Arc<FileView>> = shown
        .into_iter()
        .filter(|file| !reread.contains(&key(&file.diff.change)))
        .chain(read)
        .map(|file| (key(&file.diff.change), file))
        .collect();
    changes
        .iter()
        .filter_map(|change| files.remove(&key(change)))
        .collect()
}

/// Passes on what the watcher reports, a burst at a time: once it has been quiet a moment, or
/// has gone on for a while.
async fn follow(
    this: WeakEntity<Changes>,
    mut touched: UnboundedReceiver<Vec<Touched>>,
    cx: &mut AsyncApp,
) {
    while let Some(first) = touched.next().await {
        let mut burst = first;
        let started = Instant::now();
        loop {
            cx.background_executor().timer(QUIET).await;
            let mut quiet = true;
            while let Ok(more) = touched.try_recv() {
                burst.extend(more);
                quiet = false;
            }
            if quiet || started.elapsed() >= LONGEST_WAIT {
                break;
            }
        }
        if this.update(cx, |this, cx| this.touched(burst, cx)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use colosso_git::Content;

    use super::*;

    fn change(path: &str, status: Status) -> Change {
        Change {
            path: path.into(),
            status,
            staged: false,
        }
    }

    fn file(change: &Change) -> Arc<FileView> {
        Arc::new(FileView {
            diff: FileDiff {
                change: change.clone(),
                content: Content::Binary,
            },
            colours: OnceLock::new(),
        })
    }

    fn stale(files: &[&str]) -> Stale {
        let mut stale = Stale::default();
        stale.add(
            files
                .iter()
                .map(|path| Touched::File((*path).into()))
                .collect(),
        );
        stale
    }

    #[test]
    fn only_touched_new_and_restatused_files_are_read_again() {
        let a = change("a.rs", Status::Modified);
        let b = change("b.rs", Status::Modified);
        let c = change("c.rs", Status::Added);
        let listed = vec![a.clone(), b.clone(), change("c.rs", Status::Modified)];
        let changes = [a, b, c, change("d.rs", Status::Added)];
        let unread = unread(&changes, &listed, &stale(&["a.rs"]));
        let paths: Vec<&str> = unread.iter().map(|change| change.path.as_str()).collect();
        assert_eq!(paths, ["a.rs", "c.rs", "d.rs"]);
    }

    #[test]
    fn a_rename_is_read_again_when_its_source_is_touched() {
        let moved = change(
            "new.rs",
            Status::Renamed {
                from: "old.rs".into(),
            },
        );
        let unread = unread(
            std::slice::from_ref(&moved),
            std::slice::from_ref(&moved),
            &stale(&["old.rs"]),
        );
        assert_eq!(unread.len(), 1);
    }

    #[test]
    fn head_moving_or_a_flood_of_writes_reads_everything_again() {
        let a = change("a.rs", Status::Modified);
        let mut head = Stale::default();
        head.add(vec![Touched::Index, Touched::Head]);
        assert_eq!(
            unread(std::slice::from_ref(&a), std::slice::from_ref(&a), &head).len(),
            1
        );
        let many: Vec<String> = (0..=MOST_TOUCHED).map(|n| format!("f{n}")).collect();
        let flood = stale(&many.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(flood.everything && flood.files.is_empty());
    }

    #[test]
    fn the_index_moving_reads_again_what_is_or_was_staged() {
        let staged = |path: &str| Change {
            staged: true,
            ..change(path, Status::Modified)
        };
        let unstaged = |path: &str| change(path, Status::Modified);
        // `b.rs` was staged and now isn't, so its unstaged change has a new base.
        let listed = vec![
            staged("a.rs"),
            staged("b.rs"),
            unstaged("a.rs"),
            unstaged("b.rs"),
            unstaged("c.rs"),
        ];
        let changes = [
            staged("a.rs"),
            unstaged("a.rs"),
            unstaged("b.rs"),
            unstaged("c.rs"),
        ];
        let mut index = Stale::default();
        index.add(vec![Touched::Index]);
        assert_eq!(unread(&changes, &listed, &index), changes[..3]);
        // Writing a file leaves what's staged of it as it was.
        assert_eq!(
            unread(&changes, &listed, &stale(&["a.rs"])),
            [unstaged("a.rs")]
        );
    }

    #[test]
    fn files_read_again_replace_theirs_and_the_rest_are_kept_in_order() {
        let a = change("a.rs", Status::Modified);
        let b = change("b.rs", Status::Modified);
        let undone = change("undone.rs", Status::Modified);
        let shown = vec![file(&a), file(&b), file(&undone)];
        let kept = shown[1].clone();
        let new_a = file(&a);
        let reread: HashSet<Key> = [key(&a), key(&undone)].into();
        let files = merged(
            &[b, a, undone, change("gone-from-status.rs", Status::Added)],
            shown,
            &reread,
            vec![new_a.clone()],
        );
        assert_eq!(files.len(), 2);
        assert!(Arc::ptr_eq(&files[0], &kept));
        assert!(Arc::ptr_eq(&files[1], &new_a));
    }
}

//! What a checkout changed since its last commit, as git stages it: what's staged, diffed from
//! the commit to the index, and what isn't, diffed from the index to the files. Read in-process with gitoxide rather than through `git`, so following an agent as
//! it writes costs no process per look, and the files are diffed in parallel.

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use gix::bstr::BStr;
use gix::diff::blob::pipeline::{Mode, WorktreeRoots};
use gix::diff::blob::platform::prepare_diff::Operation;
use gix::diff::blob::platform::resource::Data;
use gix::diff::blob::sources::words;
use gix::diff::blob::{Diff, InternedInput, ResourceKind};
use gix::diff::index::Change as IndexChange;
use gix::objs::tree::EntryKind;
use gix::status::index_worktree::Item as WorktreeItem;
use gix::status::index_worktree::iter::Summary;
use gix::status::{Item, Submodule, UntrackedFiles, tree_index::TrackRenames};

use crate::command::{Error, Git, Result};

/// Files at least this big are summed up rather than diffed: nobody reads a diff that long, and
/// reading one costs memory and time the rest of the files are waiting on.
const LARGE_FILE: u64 = 8 << 20;

/// How many paths go to one git command, well under the system's limit on a command's length.
const PATHS_PER_COMMAND: usize = 500;

/// A checkout, its main one or a linked worktree, opened for reading its changes.
pub struct Checkout {
    repo: gix::ThreadSafeRepository,
    root: PathBuf,
}

/// A file changed since the checkout's last commit: staged, in the index against the commit,
/// or not, on disk against the index. A file partly staged has a change of each.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Change {
    /// Relative to the checkout, with `/` between folders.
    pub path: String,
    pub status: Status,
    pub staged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Added,
    /// On disk and never added. Only unstaged.
    Untracked,
    Deleted,
    Modified,
    /// Moved from `from`, perhaps changed as well.
    Renamed {
        from: String,
    },
    /// Merging left it with conflict markers to resolve.
    Conflicted,
}

/// A changed file and what changed in it.
#[derive(Clone, Debug)]
pub struct FileDiff {
    pub change: Change,
    pub content: Content,
}

#[derive(Clone, Debug)]
pub enum Content {
    /// Both sides as text, a missing side empty, and where they differ.
    Text {
        old: Lines,
        new: Lines,
        hunks: Vec<Hunk>,
    },
    /// One side or both aren't text.
    Binary,
    /// A side is too big to diff; `bytes` is the bigger one's size.
    TooLarge { bytes: u64 },
    /// Reading a side failed.
    Unreadable { reason: String },
}

/// Where a run of lines was replaced, as zero-based line ranges on either side. An empty range
/// is an insertion before, or a deletion after, its start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old: Range<u32>,
    pub new: Range<u32>,
}

/// A text split into lines, without their line endings.
#[derive(Clone, Debug, Default)]
pub struct Lines {
    text: String,
    /// Where each line starts and ends in `text`.
    spans: Vec<Range<u32>>,
}

impl Lines {
    pub fn new(text: impl Into<String>) -> Lines {
        let text = text.into();
        let mut spans = Vec::new();
        let mut start = 0;
        for (end, _) in text.match_indices('\n') {
            spans.push(start as u32..end as u32);
            start = end + 1;
        }
        if start < text.len() {
            spans.push(start as u32..text.len() as u32);
        }
        Lines { text, spans }
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The line at zero-based `index`, without its `\n` or `\r\n`.
    pub fn line(&self, index: usize) -> &str {
        let span = &self.spans[index];
        let line = &self.text[span.start as usize..span.end as usize];
        line.strip_suffix('\r').unwrap_or(line)
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        (0..self.len()).map(|index| self.line(index))
    }
}

/// What discarding a change does to its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discarding {
    /// Goes back to what's staged, or, discarding what's staged, to the last commit.
    Reverted,
    /// Deleted, and comes back.
    Restored,
    /// New on disk or staged as new, so with nothing to go back to: goes to the trash.
    Trashed,
    /// Conflicted, which is resolved rather than discarded.
    Kept,
}

impl Change {
    /// The paths the change touches: its own, and where a rename came from.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        let from = match &self.status {
            Status::Renamed { from } => Some(from.as_str()),
            _ => None,
        };
        std::iter::once(self.path.as_str()).chain(from)
    }

    pub fn discarding(&self) -> Discarding {
        match self.status {
            Status::Conflicted => Discarding::Kept,
            Status::Untracked | Status::Added => Discarding::Trashed,
            Status::Deleted => Discarding::Restored,
            Status::Modified | Status::Renamed { .. } => Discarding::Reverted,
        }
    }
}

impl Content {
    /// Lines added and removed, as git counts them.
    pub fn line_counts(&self) -> (u32, u32) {
        match self {
            Content::Text { hunks, .. } => hunks.iter().fold((0, 0), |(added, removed), hunk| {
                (
                    added + hunk.new.len() as u32,
                    removed + hunk.old.len() as u32,
                )
            }),
            _ => (0, 0),
        }
    }
}

impl Checkout {
    /// The checkout at `path`, which must be the top of a main checkout or a linked worktree.
    pub fn open(path: &Path) -> Result<Checkout> {
        let repo = gix::open(path).map_err(failed)?;
        let root = repo
            .workdir()
            .ok_or_else(|| Error::refused(format!("{} has no files checked out.", path.display())))?
            .to_owned();
        Ok(Checkout {
            repo: repo.into_sync(),
            root,
        })
    }

    /// The top of the checkout's files.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The checkout's own git folder, with its index and `HEAD`, and the repository's, with
    /// the branches. A main checkout's two are one.
    pub fn git_dirs(&self) -> (PathBuf, PathBuf) {
        let repo = self.repo.to_thread_local();
        (repo.git_dir().to_owned(), repo.common_dir().to_owned())
    }

    /// The files changed since the last commit: the staged ones by path, then the rest by path.
    /// Ignored files and submodules are left out; untracked files are listed each on its own
    /// rather than as their folder, and a conflicted file once, as unstaged.
    pub fn changes(&self) -> Result<Vec<Change>> {
        let repo = self.repo.to_thread_local();
        let items = repo
            .status(gix::progress::Discard)
            .map_err(failed)?
            .untracked_files(UntrackedFiles::Files)
            .index_worktree_submodules(Submodule::Given {
                ignore: gix::submodule::config::Ignore::All,
                check_dirty: false,
            })
            .index_worktree_rewrites(gix::diff::Rewrites::default())
            .tree_index_track_renames(TrackRenames::Given(gix::diff::Rewrites::default()))
            .into_iter(None)
            .map_err(failed)?;

        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        for item in items {
            let item = item.map_err(failed)?;
            let path = item.location().to_string();
            let (status, staged_or_not) = match item {
                Item::TreeIndex(change) => {
                    let status = match change {
                        IndexChange::Addition { .. } | IndexChange::Rewrite { copy: true, .. } => {
                            Status::Added
                        }
                        IndexChange::Deletion { .. } => Status::Deleted,
                        IndexChange::Modification { .. } => Status::Modified,
                        IndexChange::Rewrite {
                            source_location, ..
                        } => Status::Renamed {
                            from: source_location.to_string(),
                        },
                    };
                    (status, &mut staged)
                }
                Item::IndexWorktree(item) => {
                    let status = match (&item, item.summary()) {
                        (_, None) => continue,
                        (WorktreeItem::Rewrite { source, .. }, Some(Summary::Renamed)) => {
                            Status::Renamed {
                                from: source.rela_path().to_string(),
                            }
                        }
                        (_, Some(Summary::Removed)) => Status::Deleted,
                        (_, Some(Summary::Added | Summary::Copied)) => Status::Untracked,
                        // Added with `git add -N`: in the index, but none of its lines yet.
                        (_, Some(Summary::IntentToAdd)) => Status::Added,
                        (_, Some(Summary::Conflict)) => Status::Conflicted,
                        (_, Some(_)) => Status::Modified,
                    };
                    (status, &mut unstaged)
                }
            };
            staged_or_not.push((path, status));
        }

        // A conflict is one thing to resolve, not a change in the index as well.
        let conflicted: HashSet<String> = (unstaged.iter())
            .filter(|(_, status)| *status == Status::Conflicted)
            .map(|(path, _)| path.clone())
            .collect();
        staged.retain(|(path, _)| !conflicted.contains(path));
        let mut seen = HashSet::new();
        unstaged
            .retain(|(path, status)| *status != Status::Conflicted || seen.insert(path.clone()));
        staged.sort_by(|a, b| a.0.cmp(&b.0));
        unstaged.sort_by(|a, b| a.0.cmp(&b.0));
        let changes = |list: Vec<(String, Status)>, staged: bool| {
            (list.into_iter()).map(move |(path, status)| Change {
                path,
                status,
                staged,
            })
        };
        Ok(changes(staged, true)
            .chain(changes(unstaged, false))
            .collect())
    }

    /// Stages `changes` as their files are now: what's new or changed added, what's deleted
    /// taken out. A rename takes its source along.
    pub fn stage(&self, changes: &[Change]) -> Result<()> {
        self.git(&["update-index", "--add", "--remove"], &paths(changes))
    }

    /// Takes `changes` out of the index, back to the last commit, leaving their files as they
    /// are.
    pub fn unstage(&self, changes: &[Change]) -> Result<()> {
        self.git(&["reset", "--quiet"], &paths(changes))
    }

    /// Throws `changes` away. An unstaged change goes back to what's staged; a staged one takes
    /// the file back to the last commit, dropping what's unstaged of it too. A file that would
    /// be left with nothing to go back to goes to the trash. Conflicted files are left to
    /// resolve.
    pub fn discard(&self, changes: &[Change]) -> Result<()> {
        let mut unstage = Vec::new();
        let mut from_head = Vec::new();
        let mut from_index = Vec::new();
        let mut trash = Vec::new();
        for change in changes {
            let discarding = change.discarding();
            if discarding == Discarding::Kept {
                continue;
            }
            let restore = if change.staged {
                unstage.extend(change.paths());
                &mut from_head
            } else {
                &mut from_index
            };
            match &change.status {
                // Renamed: the new file goes and the old one comes back.
                Status::Renamed { from } => {
                    trash.push(change.path.as_str());
                    restore.push(from.as_str());
                }
                _ if discarding == Discarding::Trashed => trash.push(&change.path),
                _ => restore.push(&change.path),
            }
        }
        self.git(&["reset", "--quiet"], &unstage)?;
        self.git(&["checkout", "HEAD"], &from_head)?;
        self.git(&["checkout"], &from_index)?;
        self.trash(&trash)
    }

    /// Moves the files at `paths` to the trash, where they can be put back from.
    fn trash(&self, paths: &[&str]) -> Result<()> {
        let files: Vec<PathBuf> = (paths.iter())
            .map(|path| self.root.join(path))
            // Already gone, as a file staged as new and then deleted is.
            .filter(|path| path.symlink_metadata().is_ok())
            .collect();
        if files.is_empty() {
            return Ok(());
        }
        trash_context()
            .delete_all(&files)
            .map_err(|error| Error::refused(format!("Couldn't move to the trash: {error}")))
    }

    /// Runs git's `command` on `paths`, taken as they are rather than as patterns, a batch at a
    /// time. No paths runs nothing.
    fn git(&self, command: &[&str], paths: &[&str]) -> Result<()> {
        for batch in paths.chunks(PATHS_PER_COMMAND) {
            Git::new(&self.root)
                .arg("--literal-pathspecs")
                .args(command)
                .arg("--")
                .args(batch)
                .run()?;
        }
        Ok(())
    }

    /// What changed in each of `changes`, in their order, read on as many threads as there are
    /// cores. A file that can't be read says so in its content rather than failing the rest;
    /// one whose change undid itself, which only reading it can tell, is left out.
    pub fn diffs(&self, changes: Vec<Change>) -> Vec<FileDiff> {
        let threads = std::thread::available_parallelism()
            .map_or(1, |threads| threads.get())
            .min(changes.len())
            .max(1);
        // Each thread takes the next file when it's done with one, so a big file doesn't hold
        // up the files behind it.
        let next = AtomicUsize::new(0);
        let mut diffs: Vec<(usize, FileDiff)> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|_| scope.spawn(|| self.diff_next(&changes, &next)))
                .collect();
            workers
                .into_iter()
                // A worker that panicked would lose its files without a word, so its panic goes on.
                .flat_map(|worker| {
                    worker
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect()
        });
        diffs.sort_by_key(|(index, _)| *index);
        diffs.into_iter().map(|(_, diff)| diff).collect()
    }

    /// Diffs the `next` of `changes` until there are none left, saying where each came in them.
    fn diff_next(&self, changes: &[Change], next: &AtomicUsize) -> Vec<(usize, FileDiff)> {
        let repo = self.repo.to_thread_local();
        let mut prepared = Sides::new(&repo, &self.root);
        let mut diffs = Vec::new();
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(change) = changes.get(index) else {
                return diffs;
            };
            let content = match &mut prepared {
                Ok(sides) => {
                    let content = diff_file(&repo, sides, &self.root, change);
                    sides.clear();
                    match content {
                        Ok(content) => content,
                        Err(error) => Some(Content::Unreadable {
                            reason: error.message,
                        }),
                    }
                }
                Err(error) => Some(Content::Unreadable {
                    reason: error.message.clone(),
                }),
            };
            if let Some(content) = content {
                let change = change.clone();
                diffs.push((index, FileDiff { change, content }));
            }
        }
    }
}

/// What a thread diffs files from: the commit, the index, and a cache of the sides read for
/// each kind of change.
struct Sides<'repo> {
    head: gix::Tree<'repo>,
    index: gix::worktree::Index,
    /// Reads both sides from the repository.
    staged: gix::diff::blob::Platform,
    /// Reads the new side from the files.
    unstaged: gix::diff::blob::Platform,
}

impl<'repo> Sides<'repo> {
    fn new(repo: &'repo gix::Repository, root: &Path) -> Result<Self> {
        let cache = |new_root: Option<PathBuf>| {
            let mut cache = repo
                .diff_resource_cache(
                    // As git stores them, so a smudge filter like LFS's never runs to fetch one.
                    Mode::ToGit,
                    WorktreeRoots {
                        old_root: None,
                        new_root,
                    },
                )
                .map_err(failed)?;
            cache.filter.options.large_file_threshold_bytes = LARGE_FILE;
            Ok::<_, Error>(cache)
        };
        Ok(Sides {
            head: head_tree(repo)?,
            index: repo.index_or_empty().map_err(failed)?,
            staged: cache(None)?,
            unstaged: cache(Some(root.to_owned()))?,
        })
    }

    /// The blob at `path` in the commit and its kind, if it's there.
    fn in_head(&self, path: &str) -> Result<Option<(gix::ObjectId, EntryKind)>> {
        let entry = self.head.lookup_entry_by_path(path).map_err(failed)?;
        Ok(entry.map(|entry| (entry.object_id(), entry.mode().kind())))
    }

    /// The blob at `path` in the index and its kind, if it's there.
    fn in_index(&self, path: &str) -> Option<(gix::ObjectId, EntryKind)> {
        let entry = self.index.entry_by_path(path.into())?;
        Some((entry.id, entry.mode.to_tree_entry_mode()?.kind()))
    }

    /// Each file's sides are read once; keeping them would only grow.
    fn clear(&mut self) {
        self.staged.clear_resource_cache_keep_allocation();
        self.unstaged.clear_resource_cache_keep_allocation();
    }
}

/// Diffs `change`: staged, from the commit to the index; unstaged, from the index to its file
/// under `root`. `None` if, read in full, the change undid itself.
fn diff_file(
    repo: &gix::Repository,
    sides: &mut Sides<'_>,
    root: &Path,
    change: &Change,
) -> Result<Option<Content>> {
    let old_path = match &change.status {
        Status::Renamed { from } => from.as_str(),
        _ => change.path.as_str(),
    };
    let null = repo.object_hash().null();
    let missing = (null, EntryKind::Blob);
    let old = match (change.staged, &change.status) {
        (_, Status::Added | Status::Untracked) => None,
        // The index holds the conflict's sides, not one to diff against.
        (true, _) | (false, Status::Conflicted) => sides.in_head(old_path)?,
        (false, _) => sides.in_index(old_path),
    };
    let (old_id, old_kind) = old.unwrap_or(missing);
    let new = match (change.staged, &change.status) {
        (true, Status::Deleted) => missing,
        (true, _) => sides.in_index(&change.path).unwrap_or(missing),
        (false, _) => match root.join(&change.path).symlink_metadata() {
            Ok(meta) if meta.is_symlink() => (null, EntryKind::Link),
            Ok(meta) if is_executable(&meta) => (null, EntryKind::BlobExecutable),
            _ => missing,
        },
    };
    let (new_id, new_kind) = new;
    let cache = if change.staged {
        &mut sides.staged
    } else {
        &mut sides.unstaged
    };
    cache
        .set_resource(
            old_id,
            old_kind,
            old_path.into(),
            ResourceKind::OldOrSource,
            &repo.objects,
        )
        .map_err(failed)?;
    let new_path: &BStr = change.path.as_str().into();
    cache
        .set_resource(
            new_id,
            new_kind,
            new_path,
            ResourceKind::NewOrDestination,
            &repo.objects,
        )
        .map_err(failed)?;

    let outcome = cache.prepare_diff().map_err(failed)?;
    let algorithm = match outcome.operation {
        Operation::InternalDiff { algorithm } => algorithm,
        Operation::ExternalCommand { .. } => gix::diff::blob::Algorithm::Histogram,
        Operation::SourceOrDestinationIsBinary => {
            let size = |data: Data<'_>| match data {
                Data::Binary { size } => size,
                _ => 0,
            };
            let bytes = size(outcome.old.data).max(size(outcome.new.data));
            return Ok(Some(if bytes >= LARGE_FILE {
                Content::TooLarge { bytes }
            } else {
                Content::Binary
            }));
        }
    };
    let text = |data: Data<'_>| {
        Lines::new(String::from_utf8_lossy(data.as_slice().unwrap_or_default()).into_owned())
    };
    let (old, new) = (text(outcome.old.data), text(outcome.new.data));
    let hunks: Vec<Hunk> = {
        let mut input = InternedInput::default();
        input.update_before(old.iter());
        input.update_after(new.iter());
        let mut diff = Diff::compute(algorithm, &input);
        diff.postprocess_lines(&input);
        diff.hunks()
            .map(|hunk| Hunk {
                old: hunk.before,
                new: hunk.after,
            })
            .collect()
    };
    // Changed and changed back, staged or not: nothing to show unless its mode changed.
    if hunks.is_empty() && change.status == Status::Modified && old_kind == new_kind {
        return Ok(None);
    }
    Ok(Some(Content::Text { old, new, hunks }))
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    false
}

/// Lines longer than this are marked whole rather than word by word.
const LONGEST_WORD_DIFF: usize = 2000;

/// Pairs sharing less than this share of their text are marked whole: marking nearly every word
/// says less than marking none.
const MIN_SHARED: f32 = 0.3;

/// The byte ranges of the words that differ, in the old line and in the new.
pub type ChangedWords = (Vec<Range<usize>>, Vec<Range<usize>>);

/// Where a removed line and the added line that replaced it differ, as byte ranges in each. A
/// run of changed words with only spaces between them is one range. `None` if the lines share
/// too little, or are too long, for marking words to help.
pub fn changed_words(old: &str, new: &str) -> Option<ChangedWords> {
    if old.len() > LONGEST_WORD_DIFF || new.len() > LONGEST_WORD_DIFF {
        return None;
    }
    let (old_words, new_words): (Vec<&str>, Vec<&str>) =
        (words(old).collect(), words(new).collect());
    let mut input = InternedInput::default();
    input.update_before(old_words.iter().copied());
    input.update_after(new_words.iter().copied());
    let mut diff = Diff::compute(gix::diff::blob::Algorithm::Myers, &input);
    diff.postprocess_no_heuristic(&input);

    // The changed words' ranges, a run with only spaces between them made one.
    let ranges = |text: &str, words: &[&str], changed: &dyn Fn(u32) -> bool| {
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut start = 0;
        for (index, word) in words.iter().enumerate() {
            let end = start + word.len();
            if changed(index as u32) {
                match ranges.last_mut() {
                    Some(last) if text[last.end..start].bytes().all(|byte| byte == b' ') => {
                        last.end = end
                    }
                    _ => ranges.push(start..end),
                }
            }
            start = end;
        }
        ranges
    };
    let old_ranges = ranges(old, &old_words, &|index| diff.is_removed(index));
    let new_ranges = ranges(new, &new_words, &|index| diff.is_added(index));
    let changed: usize = old_ranges
        .iter()
        .chain(&new_ranges)
        .map(|range| range.len())
        .sum();
    let total = old.len() + new.len();
    let shared = 1. - changed as f32 / total.max(1) as f32;
    (shared >= MIN_SHARED).then_some((old_ranges, new_ranges))
}

/// Where discarded files go.
#[cfg(target_os = "macos")]
fn trash_context() -> trash::TrashContext {
    use trash::macos::{DeleteMethod, TrashContextExtMacos as _};
    let mut trash = trash::TrashContext::default();
    // Not Finder's way, the default, which asks for permission to control Finder.
    trash.set_delete_method(DeleteMethod::NsFileManager);
    trash
}

#[cfg(not(target_os = "macos"))]
fn trash_context() -> trash::TrashContext {
    trash::TrashContext::default()
}

/// The paths `changes` touch.
fn paths(changes: &[Change]) -> Vec<&str> {
    changes.iter().flat_map(Change::paths).collect()
}

/// The tree of the checked-out commit, empty before the first commit.
fn head_tree(repo: &gix::Repository) -> Result<gix::Tree<'_>> {
    let id = repo.head_tree_id_or_empty().map_err(failed)?;
    repo.find_tree(id).map_err(failed)
}

fn failed(error: impl std::fmt::Display) -> Error {
    Error::refused(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_on_newlines_without_their_endings() {
        let lines = Lines::new("a\r\nb\n\nc");
        assert_eq!(lines.iter().collect::<Vec<_>>(), ["a", "b", "", "c"]);
        assert_eq!(Lines::new("a\n").len(), 1);
        assert!(Lines::new(String::new()).is_empty());
    }

    #[test]
    #[expect(
        clippy::single_range_in_vec_init,
        reason = "one range of bytes, not its numbers"
    )]
    fn a_changed_word_is_marked_on_both_sides() {
        let (old, new) = changed_words("let x = 1;", "let total = 1;").unwrap();
        assert_eq!(old, [4..5]);
        assert_eq!(new, [4..9]);
    }

    #[test]
    #[expect(
        clippy::single_range_in_vec_init,
        reason = "one range of bytes, not its numbers"
    )]
    fn changed_words_with_only_spaces_between_are_one_range() {
        let (old, new) = changed_words("fn a b c(x)", "fn d e f(x)").unwrap();
        assert_eq!(old, [3..8]);
        assert_eq!(new, [3..8]);
    }

    #[test]
    fn lines_with_little_in_common_are_marked_whole() {
        assert_eq!(changed_words("struct Payment {", "// nothing alike"), None);
        assert_eq!(changed_words(&"a".repeat(3000), "b"), None);
    }
}

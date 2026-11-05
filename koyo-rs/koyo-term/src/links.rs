//! Links in a terminal: the URLs and file paths a program prints, and the links it makes of
//! its text (OSC 8), which holding ⌘ underlines under the pointer and ⌘-click opens. A path
//! is a link only when it names a file that's there: relative ones are looked for in the
//! shell's folder (OSC 7), then in the folders the view is given, the terminal's checkout.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use koyo_term_engine::{Frame, Grapheme, GridPoint, Width};
use percent_encoding::percent_decode_str;

mod hover;
mod scan;
#[cfg(test)]
mod tests;

/// How long what was learned of a path stays known, so a file made since is found.
const FORGET: Duration = Duration::from_secs(2);

/// How many paths are kept known before they're all forgotten.
const KEPT: usize = 256;

/// What a link in a terminal opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// A file, at the line and column the link names, counted from 1 as printed.
    File {
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    },
    /// A web page, for the browser.
    Url(String),
}

/// A link in the frame: what it opens and the cells it covers, a run of columns per row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    pub target: LinkTarget,
    pub cells: Vec<(u16, Range<u16>)>,
}

/// What a view keeps to find links: the folders a relative path may be in besides the
/// shell's, which paths it found to be files lately, and the link the pointer is over while
/// ⌘ is held.
#[derive(Default)]
pub(crate) struct Links {
    roots: Vec<PathBuf>,
    files: Files,
    /// The cell the pointer is over while ⌘ is held over the grid.
    pointer: Option<GridPoint>,
    /// The cell and frame a link was last looked for at.
    looked: Option<(GridPoint, u64)>,
    pub hovered: Option<Rc<Found>>,
}

impl Links {
    /// The link at `at` in `frame`.
    pub fn find(&mut self, frame: &Frame, at: GridPoint) -> Option<Found> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let Links { roots, files, .. } = self;
        find(frame, at, roots, home.as_deref(), &mut |path| files.is_file(path))
    }
}

/// Which paths are files, as they were lately: hovering asks the disk once per path, not at
/// every move of the pointer.
#[derive(Default)]
struct Files {
    known: HashMap<PathBuf, bool>,
    since: Option<Instant>,
}

impl Files {
    fn is_file(&mut self, path: &Path) -> bool {
        let stale = self.since.is_none_or(|since| since.elapsed() > FORGET);
        if stale || self.known.len() >= KEPT {
            self.known.clear();
            self.since = Some(Instant::now());
        }
        if let Some(&known) = self.known.get(path) {
            return known;
        }
        let file = path.is_file();
        self.known.insert(path.to_path_buf(), file);
        file
    }
}

/// The link at `at` in `frame`: the program's own (OSC 8) when the cell is in one that
/// opens a page or a file, else a URL or a path to a file in the text around it. A relative
/// path is looked for in the shell's folder, then in `roots`; `home` is what `~` means and
/// `is_file` says which paths are files.
pub(crate) fn find(
    frame: &Frame,
    at: GridPoint,
    roots: &[PathBuf],
    home: Option<&Path>,
    is_file: &mut dyn FnMut(&Path) -> bool,
) -> Option<Found> {
    let line = Line::of(frame, at.row);
    let index = line.index_of(at)?;
    let pwd = shell_folder(&frame.pwd);
    let mut file = |shown: &str| resolve(shown, pwd.as_deref(), roots, home, is_file);
    if let Some((range, uri)) = line.link_at(frame, index)
        && let Some(target) = scan::target(uri, &mut file)
    {
        return Some(Found {
            target,
            cells: line.cells(range),
        });
    }
    let (range, target) = scan::find(&line.text, index, &mut file)?;
    Some(Found {
        target,
        cells: line.cells(range),
    })
}

/// What a link's target names, as a chat writes one in Markdown: a web page, or a file that's
/// there, at the line it names (`#L42`, `:42`, `:42:7`). A path may be percent-escaped, and is
/// absolute, from `~`, or relative to the first of `folders` it's in.
pub fn link_target(target: &str, folders: &[PathBuf]) -> Option<LinkTarget> {
    let target = target.trim();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut file =
        |shown: &str| resolve(shown, None, folders, home.as_deref(), &mut |path| path.is_file());
    if target.contains("://") {
        return scan::target(target, &mut file);
    }
    // `a.rs#L42` as GitHub names a line; another fragment, a heading's, names no place.
    let (path, line) = match target.split_once('#') {
        Some((path, fragment)) => (path, fragment_line(fragment)),
        None => (target, None),
    };
    let path = percent_decode_str(path).decode_utf8().ok()?;
    scan::located(&path, &mut file).map(|(_, found)| at_line(found, line))
}

/// `found`, at `line` when it's a file whose own text names none.
fn at_line(found: LinkTarget, line: Option<u32>) -> LinkTarget {
    match found {
        LinkTarget::File { path, line: None, column } => LinkTarget::File { path, line, column },
        found => found,
    }
}

/// The file `shown` names, if it's there: `~/` is the home folder, and a relative path is
/// looked for in `pwd`, then in each of `roots`.
fn resolve(
    shown: &str,
    pwd: Option<&Path>,
    roots: &[PathBuf],
    home: Option<&Path>,
    is_file: &mut dyn FnMut(&Path) -> bool,
) -> Option<PathBuf> {
    if shown.is_empty() {
        return None;
    }
    if let Some(rest) = shown.strip_prefix("~/") {
        return home.map(|home| tidied(&home.join(rest))).filter(|path| is_file(path));
    }
    let path = Path::new(shown);
    if path.is_absolute() {
        return Some(tidied(path)).filter(|path| is_file(path));
    }
    (pwd.into_iter().chain(roots.iter().map(PathBuf::as_path)))
        .map(|root| tidied(&root.join(path)))
        .find(|path| is_file(path))
}

/// `path` as a tab names it: without its `.` folders, and each `..` taking the folder before
/// it away, as written, so a path under a checkout stays under its root as the tree has it.
fn tidied(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut tidy = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(tidy.components().next_back(), Some(Component::Normal(_))) =>
            {
                tidy.pop();
            }
            _ => tidy.push(part),
        }
    }
    tidy
}

/// The shell's folder, as its OSC 7 says it: a `file://` URI, or the `kitty-shell-cwd://` one
/// Ghostty's zsh integration sends, its path as it is.
fn shell_folder(pwd: &str) -> Option<PathBuf> {
    if let Some(rest) = pwd.strip_prefix("kitty-shell-cwd://") {
        return Some(PathBuf::from(rest.get(rest.find('/')?..)?));
    }
    file_uri(pwd).map(|(path, _)| PathBuf::from(path))
}

/// The path a `file://` URI names, decoded, and the line its fragment names (`#L42`).
pub(crate) fn file_uri(uri: &str) -> Option<(String, Option<u32>)> {
    let rest = uri.strip_prefix("file://")?;
    // The host, if any, is the machine's own: the path starts at the first slash.
    let path = rest.get(rest.find('/')?..)?;
    let (path, line) = match path.split_once('#') {
        Some((path, fragment)) => (path, fragment_line(fragment)),
        None => (path, None),
    };
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let path = percent_decode_str(path).decode_utf8().ok()?;
    Some((path.into_owned(), line))
}

/// The line a fragment names, as `#L42` and `#L42-L50` do.
fn fragment_line(fragment: &str) -> Option<u32> {
    let line = fragment.strip_prefix('L')?;
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    line.get(..digits)?.parse().ok()
}

/// A line as the program wrote it, the rows its soft wraps join, as characters, each with
/// the cell it's shown in: its row and column, and how many columns it takes.
struct Line {
    text: Vec<char>,
    cells: Vec<(u16, u16, u16)>,
}

impl Line {
    fn of(frame: &Frame, row: u16) -> Line {
        let first = frame.line_start(row);
        let mut line = Line {
            text: Vec::new(),
            cells: Vec::new(),
        };
        for (row, r) in frame.rows.iter().skip(usize::from(first)).zip(first..) {
            for (cell, c) in row.cells.iter().zip(0u16..) {
                let width = match cell.width {
                    // A wide character's tail, and the blank a wide one that didn't fit left.
                    Width::SpacerTail | Width::SpacerHead => continue,
                    Width::Wide => 2,
                    Width::Narrow => 1,
                };
                let mut push = |ch: char| {
                    line.text.push(ch);
                    line.cells.push((r, c, width));
                };
                match &cell.grapheme {
                    Grapheme::Empty => push(' '),
                    Grapheme::Char(ch) => push(*ch),
                    Grapheme::Cluster(cluster) => cluster.chars().for_each(push),
                }
            }
            if !row.wrapped {
                break;
            }
        }
        line
    }

    /// The character shown in the cell at `at`, or in the wide cell it's the tail of.
    fn index_of(&self, at: GridPoint) -> Option<usize> {
        (self.cells.iter())
            .position(|&(row, col, width)| row == at.row && col <= at.col && at.col < col + width)
    }

    /// The runs of cells characters `range` are shown in, one per row.
    fn cells(&self, range: Range<usize>) -> Vec<(u16, Range<u16>)> {
        let mut runs: Vec<(u16, Range<u16>)> = Vec::new();
        for &(row, col, width) in self.cells.get(range).unwrap_or_default() {
            match runs.last_mut() {
                Some((last, cols)) if *last == row => cols.end = col + width,
                _ => runs.push((row, col..col + width)),
            }
        }
        runs
    }

    /// The program's link (OSC 8) character `index` is in: the characters it covers on
    /// this line, rows it wraps onto included, and its URI.
    fn link_at<'f>(&self, frame: &'f Frame, index: usize) -> Option<(Range<usize>, &'f str)> {
        let uri = self.uri_at(frame, index)?;
        let same = |i: &usize| self.uri_at(frame, *i) == Some(uri);
        let start = (0..index).rev().take_while(same).last().unwrap_or(index);
        let end = (index + 1..self.text.len())
            .take_while(same)
            .last()
            .map_or(index + 1, |last| last + 1);
        Some((start..end, uri))
    }

    fn uri_at<'f>(&self, frame: &'f Frame, index: usize) -> Option<&'f str> {
        let &(row, col, _) = self.cells.get(index)?;
        let links = &frame.rows.get(usize::from(row))?.links;
        (links.iter())
            .find(|link| link.cols.contains(&col))
            .map(|link| &*link.uri)
    }
}

//! The places a symbol leads to, for the Files panel to list under the editor: every
//! reference to it (`textDocument/references`), or its several definitions. Each place comes
//! with the line it's on, taken from the text the server has for the file when the file's
//! open on it, and else from the disk, off the main thread.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use colosso_lsp::lsp_types::request::References;
use colosso_lsp::lsp_types::{
    self, LocationLink, PartialResultParams, Position, ReferenceContext, ReferenceParams,
    WorkDoneProgressParams,
};
use colosso_lsp::{Encoding, IndexedText, uri};
use gpui_kit::component::input::{Rope, RopeExt as _};
use gpui_kit::{App, AppContext as _, AsyncApp, SharedString, Task, WeakEntity};

use super::providers::Hooks;
use super::server::Server;
use crate::checkout::{Content, MOST_BYTES, read_at_most};
use crate::files::At;

/// What a symbol led to, as the panel lists it. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Found {
    /// What was looked for and how much was found: "3 references to `greet`".
    pub title: SharedString,
    /// The files with places in them, in the order the server gave them.
    pub files: Arc<[FileLocations]>,
}

/// The places in one file, by line.
#[derive(Debug)]
pub struct FileLocations {
    pub path: PathBuf,
    /// As the tree names it; the whole path for a file outside the checkout.
    pub relative: SharedString,
    pub rows: Vec<Located>,
}

/// One place, and the line it's on.
#[derive(Debug)]
pub struct Located {
    /// Counted from 0.
    pub line: u32,
    /// The line, its indentation and trailing space dropped, cut down around the place
    /// when it's very long.
    pub text: SharedString,
    /// The bytes of `text` the place covers.
    pub matched: Range<usize>,
    /// Where in the file the place is: its bytes, or only its line when the file couldn't
    /// be read.
    pub at: At,
}

/// The places in each file, in the server's encoding, the files in the order first met.
type Places = Vec<(PathBuf, Vec<lsp_types::Range>)>;

/// The longest line shown whole; a longer one is cut down around the place.
const LONGEST_LINE: usize = 240;
/// How much of a long line is kept before the place.
const BEFORE: usize = 40;

impl Hooks {
    /// Every reference to the symbol at `offset` of `rope`, the declaration included.
    pub(super) fn references(&self, rope: &Rope, offset: usize, cx: &mut App) -> Task<Option<Found>> {
        let Some(asked) = self.ask(rope, cx) else {
            return Task::ready(None);
        };
        let name = rope.word_at(offset);
        let server = self.server.clone();
        let root = self.root(cx);
        cx.spawn(async move |cx| {
            let places = cx
                .background_spawn(async move {
                    let params = ReferenceParams {
                        text_document_position: asked.at(offset),
                        work_done_progress_params: WorkDoneProgressParams::default(),
                        partial_result_params: PartialResultParams::default(),
                        context: ReferenceContext {
                            include_declaration: true,
                        },
                    };
                    let found = asked.ask::<References>(&params).await.flatten();
                    grouped((found.unwrap_or_default().into_iter()).filter_map(|location| {
                        Some((uri::to_path(&location.uri)?, location.range))
                    }))
                })
                .await;
            let title = format!("{} to `{name}`", counted(&places, "reference"));
            Some(Found {
                title: title.into(),
                files: located(places, &server, root, cx).await,
            })
        })
    }

    /// The several definitions `links` name, for the user to pick from; `symbol` is the
    /// bytes of `rope`, the editor's text, they're the definitions of.
    pub(super) fn definitions(
        &self,
        links: Vec<LocationLink>,
        rope: &Rope,
        symbol: Range<usize>,
        cx: &mut App,
    ) -> Task<Option<Found>> {
        let Some(asked) = self.ask(rope, cx) else {
            return Task::ready(None);
        };
        let name = rope.slice(symbol).to_string();
        let places = grouped(links.into_iter().filter_map(|link| {
            let path = uri::to_path(&link.target_uri)?;
            let range = link.target_selection_range;
            // This file's ranges were made the editor's own, as its definitions provider
            // gives them (`link_for_editor`): back to the server's, as the others are.
            let range = if path == *self.path {
                let start = rope.position_to_offset(&range.start);
                asked.for_server(start..rope.position_to_offset(&range.end))
            } else {
                range
            };
            Some((path, range))
        }));
        let title = format!("{} of `{name}`", counted(&places, "definition"));
        let server = self.server.clone();
        let root = self.root(cx);
        cx.spawn(async move |cx| {
            Some(Found {
                title: title.into(),
                files: located(places, &server, root, cx).await,
            })
        })
    }

    /// The checkout's root, while the checkout's there.
    fn root(&self, cx: &App) -> Option<PathBuf> {
        Some(self.checkout.upgrade()?.read(cx).root().clone())
    }
}

/// `places` by file, the files in the order first met, each file's in the order given.
fn grouped(places: impl Iterator<Item = (PathBuf, lsp_types::Range)>) -> Places {
    let mut files: Places = Vec::new();
    let mut index: HashMap<PathBuf, usize> = HashMap::new();
    for (path, range) in places {
        let at = *index.entry(path.clone()).or_insert(files.len());
        if at == files.len() {
            files.push((path, Vec::new()));
        }
        files[at].1.push(range);
    }
    files
}

/// How many `places` there are, in words: "1 reference", "3 references", "no definitions".
fn counted(places: &Places, what: &str) -> String {
    match places.iter().map(|(_, ranges)| ranges.len()).sum() {
        0 => format!("no {what}s"),
        1 => format!("1 {what}"),
        n => format!("{n} {what}s"),
    }
}

/// `files`' places, each with its line's text, sorted by line within a file; `root` is the
/// checkout's, which the files are named from.
async fn located(
    files: Places,
    server: &WeakEntity<Server>,
    root: Option<PathBuf>,
    cx: &mut AsyncApp,
) -> Arc<[FileLocations]> {
    // The texts the server has, read on the main thread; the rest are read from disk off it.
    let (texts, encoding) = cx.update(|cx| {
        let server = server.upgrade();
        let server = server.as_ref().map(|server| server.read(cx));
        let texts: Vec<Option<Arc<IndexedText>>> =
            (files.iter()).map(|(path, _)| server?.shown_text(path)).collect();
        (texts, server.map_or(Encoding::default(), |server| server.encoding))
    });
    cx.background_spawn(async move {
        (files.into_iter().zip(texts))
            .map(|((path, ranges), text)| {
                let text = text.or_else(|| read(&path).map(Arc::new));
                let mut rows: Vec<Located> = (ranges.into_iter())
                    .map(|range| locate(text.as_deref(), range, encoding))
                    .collect();
                rows.sort_by_key(|row| (row.line, row.matched.start));
                let relative = (root.as_deref())
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(path.as_path());
                FileLocations {
                    relative: relative.display().to_string().into(),
                    path,
                    rows,
                }
            })
            .collect()
    })
    .await
}

/// The file at `path` as text, as the editor would read it, unless it's binary, too large
/// or can't be read.
fn read(path: &Path) -> Option<IndexedText> {
    match read_at_most(path, MOST_BYTES) {
        Content::Text(text) => Some(IndexedText::new(text.to_string())),
        _ => None,
    }
}

/// The place `range` of `text` names, with the line it's on; only the line when there's
/// no text to read it from.
fn locate(text: Option<&IndexedText>, range: lsp_types::Range, encoding: Encoding) -> Located {
    let line = range.start.line;
    let Some(text) = text else {
        return Located {
            line,
            text: SharedString::default(),
            matched: 0..0,
            at: At::Line(line),
        };
    };
    let bytes = text.offsets(range, encoding);
    let s = text.as_str();
    let line_start = text.offset(Position::new(line, 0), encoding);
    let line_end = text.offset(Position::new(line, u32::MAX), encoding);
    let line_text = &s[line_start..line_end];
    let from = line_start + (line_text.len() - line_text.trim_start().len());
    let to = line_start + line_text.trim_end().len();
    let (from, to) = window(s, from.min(bytes.start), to.max(bytes.start), bytes.start);
    let start = bytes.start.clamp(from, to);
    let end = bytes.end.clamp(start, to);
    Located {
        line,
        text: s[from..to].into(),
        matched: start - from..end - from,
        at: At::Bytes(bytes),
    }
}

/// The bytes `from..to` of `s`, cut down around `at` when they're more than a line's worth,
/// on character boundaries.
fn window(s: &str, from: usize, to: usize, at: usize) -> (usize, usize) {
    if to - from <= LONGEST_LINE {
        return (from, to);
    }
    let start = s.floor_char_boundary(at.saturating_sub(BEFORE).max(from));
    let end = s.floor_char_boundary((start + LONGEST_LINE).min(to));
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_comes_with_its_line_trimmed_and_the_match_within_it() {
        let text = IndexedText::new("fn a() {}\n    let x = greet(1);  \r\n".to_owned());
        let range = lsp_types::Range::new(Position::new(1, 12), Position::new(1, 17));
        let located = locate(Some(&text), range, Encoding::Utf16);
        assert_eq!(located.line, 1);
        assert_eq!(located.text.as_ref(), "let x = greet(1);");
        assert_eq!(&located.text[located.matched.clone()], "greet");
        assert_eq!(located.at, At::Bytes(22..27));
        // Without the text, only the line is known.
        let blind = locate(None, range, Encoding::Utf16);
        assert_eq!(blind.at, At::Line(1));
        assert!(blind.text.is_empty());
    }

    #[test]
    fn a_long_line_is_cut_down_around_the_place() {
        let long = format!("{}needle{}", "é".repeat(200), "x".repeat(300));
        let text = IndexedText::new(long.clone());
        let start = long.find("needle").unwrap();
        let range = text.range(start..start + 6, Encoding::Utf8);
        let located = locate(Some(&text), range, Encoding::Utf8);
        assert_eq!(located.text.len(), LONGEST_LINE);
        assert_eq!(&located.text[located.matched.clone()], "needle");
    }

    #[test]
    fn places_are_grouped_by_file_in_the_order_met_and_counted_in_words() {
        let (a, b) = (PathBuf::from("/a.rs"), PathBuf::from("/b.rs"));
        let at = |line| lsp_types::Range::new(Position::new(line, 0), Position::new(line, 1));
        let places = grouped([(b.clone(), at(3)), (a.clone(), at(1)), (b.clone(), at(0))].into_iter());
        assert_eq!(places[0].0, b);
        assert_eq!(places[0].1.len(), 2);
        assert_eq!(places[1].0, a);
        assert_eq!(counted(&places, "reference"), "3 references");
        assert_eq!(counted(&places[1..].to_vec(), "definition"), "1 definition");
        assert_eq!(counted(&Vec::new(), "definition"), "no definitions");
    }
}

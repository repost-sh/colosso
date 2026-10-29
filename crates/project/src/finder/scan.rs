//! Matching a pattern against files, in blocks shared among a finder's threads, each block's
//! matches kept in the order its files come in.

use nucleo_matcher::pattern::Pattern;
use nucleo_matcher::{Matcher, Utf32Str};

use super::pool::{Pool, matching};
use crate::files::Folder;

/// Files, or earlier matches, a thread takes at a time.
const BLOCK: usize = 2048;

/// A score's mark for a match within the file's name, which ranks above any match that
/// needs the folders: the name decides first.
pub(super) const NAMED: u32 = 1 << 30;

/// Whether the search a scan is part of has given way to a later one, and the scan can stop.
pub(super) type GaveWay<'a> = &'a (dyn Fn() -> bool + Sync);

/// A file that matched: the folder it's in and where among that folder's files, how well,
/// and its path's length, which ranks without reading the path. A search's hits are in the
/// folders' order, and each folder's files' order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Hit {
    pub folder: u32,
    pub file: u32,
    pub score: u32,
    pub length: u32,
}

impl Hit {
    pub(super) fn spot(self) -> (u32, u32) {
        (self.folder, self.file)
    }

    pub(super) fn named(self) -> bool {
        self.score & NAMED != 0
    }
}

/// Where a folder's files' names start in their paths.
pub(super) fn name_start(folder: &Folder) -> usize {
    if folder.path.is_empty() {
        0
    } else {
        folder.path.len() + 1
    }
}

/// How well `pattern` matches the file at `path`, whose name starts at `name`: within the
/// name if it can, and it's `named` worth trying, marked `NAMED`; or else across the whole
/// path.
fn score(
    pattern: &Pattern,
    (path, name, named): (&str, usize, bool),
    matcher: &mut Matcher,
    chars: &mut Vec<char>,
) -> Option<u32> {
    if named {
        let in_name = path.get(name..).unwrap_or(path);
        if let Some(score) = pattern.score(Utf32Str::new(in_name, chars), matcher) {
            return Some(NAMED | score);
        }
    }
    pattern.score(Utf32Str::new(path, chars), matcher)
}

/// Every file of the folders at `which`, ascending, that `pattern` matches, unless the
/// search `gave_way` first.
pub(super) fn folders(
    folders: &[Folder],
    which: &[u32],
    pattern: &Pattern,
    pool: &Pool,
    gave_way: GaveWay<'_>,
) -> Vec<Hit> {
    // Where each folder's files start among all of theirs, so a block can start mid-folder.
    let mut starts = Vec::with_capacity(which.len());
    let mut total = 0;
    for &at in which {
        starts.push(total);
        total += folders
            .get(at as usize)
            .map_or(0, |folder| folder.files.len());
    }
    let blocks = (0..total.div_ceil(BLOCK)).collect();
    let found = pool.each(blocks, |block| {
        let mut hits = Vec::new();
        if gave_way() {
            return hits;
        }
        let (start, end) = (block * BLOCK, ((block + 1) * BLOCK).min(total));
        let mut nth = starts.partition_point(|&begins| begins <= start) - 1;
        let mut file = start - starts[nth];
        let mut left = end - start;
        matching(|matching| {
            while left > 0 {
                let Some(folder) = folders.get(which[nth] as usize) else {
                    return;
                };
                let name = name_start(folder);
                for (index, path) in folder.files.iter().enumerate().skip(file).take(left) {
                    let file = (&**path, name, true);
                    if let Some(score) =
                        score(pattern, file, &mut matching.matcher, &mut matching.chars)
                    {
                        hits.push(Hit {
                            folder: which[nth],
                            file: index as u32,
                            score,
                            length: path.len() as u32,
                        });
                    }
                    left -= 1;
                }
                (nth, file) = (nth + 1, 0);
            }
        });
        hits
    });
    found.concat()
}

/// Those of `earlier`, hits of a pattern `pattern` narrows, that it matches too, unless the
/// search `gave_way` first. A file the earlier pattern matched only across its folders can't
/// match within its name now.
pub(super) fn hits(
    folders: &[Folder],
    earlier: &[Hit],
    pattern: &Pattern,
    pool: &Pool,
    gave_way: GaveWay<'_>,
) -> Vec<Hit> {
    let found = pool.each(earlier.chunks(BLOCK).collect(), |block| {
        let mut hits = Vec::new();
        if gave_way() {
            return hits;
        }
        matching(|matching| {
            for hit in block {
                let Some(folder) = folders.get(hit.folder as usize) else {
                    continue;
                };
                let Some(path) = folder.files.get(hit.file as usize) else {
                    continue;
                };
                let file = (&**path, name_start(folder), hit.named());
                if let Some(score) =
                    score(pattern, file, &mut matching.matcher, &mut matching.chars)
                {
                    hits.push(Hit { score, ..*hit });
                }
            }
        });
        hits
    });
    found.concat()
}

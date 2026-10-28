//! The changed files as one flat list of rows, so a virtualised list lays out only what's on
//! screen. A row names lines by number and the view reads their text from the file's diff.
//!
//! Unchanged lines show a few at a time around each change; longer runs fold into one row that
//! opens on a click. Unified mode lists a hunk's removed lines, then its added ones; split mode
//! pairs them side by side, the nth removed beside the nth added.

use std::collections::{HashMap, HashSet};

use colosso_git::{Content, FileDiff, Hunk};

use super::state::{Key, key};

/// Unchanged lines shown before and after each change.
const CONTEXT: u32 = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Layout {
    #[default]
    Unified,
    Split,
}

/// One row of the list. Line numbers are zero-based; the view adds one to show them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The file's header, at the top of its card.
    File(usize),
    /// In place of lines: binary, too large, unreadable, or a change with no lines.
    Note(usize),
    /// `len` unchanged lines, starting at `old` and `new`, folded away.
    Fold {
        file: usize,
        old: u32,
        new: u32,
        len: u32,
    },
    /// Unified: an unchanged line with both numbers, a removed one with only `old`, an added
    /// one with only `new`.
    Line {
        file: usize,
        old: Option<u32>,
        new: Option<u32>,
    },
    /// Split: the old side's line and the new side's, either missing where the other side has
    /// more. `changed` tells a replaced pair from an unchanged line shown on both sides.
    Pair {
        file: usize,
        old: Option<u32>,
        new: Option<u32>,
        changed: bool,
    },
    /// The bottom of a file's card.
    End(usize),
}

impl Row {
    /// The file the row belongs to.
    pub fn file(self) -> usize {
        match self {
            Row::File(file) | Row::Note(file) | Row::End(file) => file,
            Row::Fold { file, .. } | Row::Line { file, .. } | Row::Pair { file, .. } => file,
        }
    }
}

/// What the user opened or closed, by file rather than by row so it outlasts the diff being
/// read again.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    /// Files folded down to their header.
    pub collapsed: HashSet<Key>,
    /// Folds opened, by file and the fold's first new-side line.
    pub unfolded: HashMap<Key, HashSet<u32>>,
}

/// The rows for `files`, laid out as `layout` with what `shown` opened or closed.
pub fn rows<'a>(
    files: impl IntoIterator<Item = &'a FileDiff>,
    layout: Layout,
    shown: &Shown,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for (file, diff) in files.into_iter().enumerate() {
        rows.push(Row::File(file));
        let key = key(&diff.change);
        if shown.collapsed.contains(&key) {
            continue;
        }
        match &diff.content {
            Content::Text { old, new, hunks } if !hunks.is_empty() => {
                let lines = (old.len() as u32, new.len() as u32);
                let unfolded = shown.unfolded.get(&key);
                file_rows(&mut rows, file, lines, hunks, layout, unfolded);
            }
            _ => rows.push(Row::Note(file)),
        }
        rows.push(Row::End(file));
    }
    rows
}

fn file_rows(
    rows: &mut Vec<Row>,
    file: usize,
    (old_len, new_len): (u32, u32),
    hunks: &[Hunk],
    layout: Layout,
    unfolded: Option<&HashSet<u32>>,
) {
    let unchanged = |rows: &mut Vec<Row>, old: u32, new: u32| {
        rows.push(match layout {
            Layout::Unified => Row::Line {
                file,
                old: Some(old),
                new: Some(new),
            },
            Layout::Split => Row::Pair {
                file,
                old: Some(old),
                new: Some(new),
                changed: false,
            },
        })
    };
    // Shows the unchanged run of `len` lines from `old`/`new`, keeping `before` lines at its
    // start and `after` at its end and folding the middle if there's more than one line to it.
    let run = |rows: &mut Vec<Row>, old: u32, new: u32, len: u32, before: u32, after: u32| {
        let kept = before + after;
        if len <= kept + 1 || unfolded.is_some_and(|unfolded| unfolded.contains(&(new + before))) {
            (0..len).for_each(|n| unchanged(rows, old + n, new + n));
            return;
        }
        (0..before).for_each(|n| unchanged(rows, old + n, new + n));
        rows.push(Row::Fold {
            file,
            old: old + before,
            new: new + before,
            len: len - kept,
        });
        (len - after..len).for_each(|n| unchanged(rows, old + n, new + n));
    };

    let (mut old, mut new) = (0, 0);
    for (index, hunk) in hunks.iter().enumerate() {
        let len = hunk.new.start - new;
        let before = if index == 0 { 0 } else { CONTEXT.min(len) };
        run(rows, old, new, len, before, CONTEXT.min(len - before));
        match layout {
            Layout::Unified => {
                rows.extend(hunk.old.clone().map(|line| Row::Line {
                    file,
                    old: Some(line),
                    new: None,
                }));
                rows.extend(hunk.new.clone().map(|line| Row::Line {
                    file,
                    old: None,
                    new: Some(line),
                }));
            }
            Layout::Split => {
                let pairs = hunk.old.len().max(hunk.new.len()) as u32;
                rows.extend((0..pairs).map(|n| Row::Pair {
                    file,
                    old: Some(hunk.old.start + n).filter(|line| *line < hunk.old.end),
                    new: Some(hunk.new.start + n).filter(|line| *line < hunk.new.end),
                    changed: true,
                }));
            }
        }
        (old, new) = (hunk.old.end, hunk.new.end);
    }
    debug_assert_eq!(new_len - new, old_len - old);
    let len = new_len - new;
    run(rows, old, new, len, CONTEXT.min(len), 0);
}

#[cfg(test)]
mod tests {
    use colosso_git::{Change, Lines, Status};

    use super::*;

    /// A modified file of `old` lines whose diff is `hunks`, the new side as long as they make it.
    fn diff(
        path: &str,
        old: u32,
        hunks: &[(std::ops::Range<u32>, std::ops::Range<u32>)],
    ) -> FileDiff {
        let added: u32 = hunks
            .iter()
            .map(|(old, new)| new.len() as u32 - old.len() as u32)
            .sum();
        let text = |len: u32| (0..len).map(|n| format!("{n}\n")).collect::<String>();
        let content = Content::Text {
            old: Lines::new(text(old)),
            new: Lines::new(text(old + added)),
            hunks: hunks
                .iter()
                .map(|(old, new)| Hunk {
                    old: old.clone(),
                    new: new.clone(),
                })
                .collect(),
        };
        FileDiff {
            change: Change {
                path: path.into(),
                status: Status::Modified,
                staged: false,
            },
            content,
        }
    }

    fn line(old: Option<u32>, new: Option<u32>) -> Row {
        Row::Line { file: 0, old, new }
    }

    #[test]
    fn a_long_unchanged_run_folds_beyond_the_context() {
        // Line 10 of 20 replaced.
        let files = [diff("a", 20, &[(10..11, 10..11)])];
        let rows = rows(&files, Layout::Unified, &Shown::default());
        let mut expected = vec![
            Row::File(0),
            Row::Fold {
                file: 0,
                old: 0,
                new: 0,
                len: 7,
            },
        ];
        expected.extend((7..10).map(|n| line(Some(n), Some(n))));
        expected.extend([line(Some(10), None), line(None, Some(10))]);
        expected.extend((11..14).map(|n| line(Some(n), Some(n))));
        expected.push(Row::Fold {
            file: 0,
            old: 14,
            new: 14,
            len: 6,
        });
        expected.push(Row::End(0));
        assert_eq!(rows, expected);
    }

    #[test]
    fn a_fold_of_one_line_shows_the_line_instead() {
        // Seven unchanged lines between two changes: three kept on each side, one in between.
        let files = [diff("a", 10, &[(0..1, 0..1), (8..9, 8..9)])];
        let rows = rows(&files, Layout::Unified, &Shown::default());
        assert!(
            !rows[1..]
                .iter()
                .any(|row| matches!(row, Row::Fold { new, .. } if *new < 8))
        );
        let unchanged = rows
            .iter()
            .filter(|row| {
                matches!(
                    row,
                    Row::Line {
                        old: Some(_),
                        new: Some(_),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(unchanged, 7 + 1);
    }

    #[test]
    fn an_opened_fold_shows_its_lines() {
        let files = [diff("a", 20, &[(10..11, 10..11)])];
        let mut shown = Shown::default();
        shown.unfolded.insert((false, "a".into()), [0].into());
        let rows = rows(&files, Layout::Unified, &shown);
        assert_eq!(rows[1], line(Some(0), Some(0)));
        assert_eq!(
            rows.iter()
                .filter(|row| matches!(row, Row::Fold { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn split_pairs_the_nth_removed_line_with_the_nth_added() {
        // Two lines replaced by three.
        let files = [diff("a", 2, &[(0..2, 0..3)])];
        let rows = rows(&files, Layout::Split, &Shown::default());
        let pair = |old, new| Row::Pair {
            file: 0,
            old,
            new,
            changed: true,
        };
        assert_eq!(
            rows,
            [
                Row::File(0),
                pair(Some(0), Some(0)),
                pair(Some(1), Some(1)),
                pair(None, Some(2)),
                Row::End(0),
            ]
        );
    }

    #[test]
    fn a_collapsed_file_shows_only_its_header() {
        let files = [diff("a", 3, &[(1..2, 1..2)]), diff("b", 3, &[(1..2, 1..2)])];
        let mut shown = Shown::default();
        shown.collapsed.insert((false, "a".into()));
        let rows = rows(&files, Layout::Unified, &shown);
        assert_eq!(rows[..2], [Row::File(0), Row::File(1)]);
    }

    #[test]
    fn a_file_without_lines_shows_a_note() {
        let mut binary = diff("logo.png", 0, &[]);
        binary.content = Content::Binary;
        let rows = rows(&[binary], Layout::Unified, &Shown::default());
        assert_eq!(rows, [Row::File(0), Row::Note(0), Row::End(0)]);
    }
}

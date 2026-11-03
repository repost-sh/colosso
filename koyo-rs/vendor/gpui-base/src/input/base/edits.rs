//! The edits made to the text, kept for a while so that what was computed
//! against an older version of the text can be brought up to date: a
//! diagnostic's range, or a range a language server answered for.

use std::collections::VecDeque;
use std::ops::Range;

use sum_tree::Bias;

/// One replacement of the text: the bytes it replaced, in the text as it
/// stood before, and what it put there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub old: Range<usize>,
    pub new_text: String,
}

impl Edit {
    pub fn new(old: Range<usize>, new_text: impl Into<String>) -> Self {
        Self {
            old,
            new_text: new_text.into(),
        }
    }

    /// The length of the text put in.
    pub fn new_len(&self) -> usize {
        self.new_text.len()
    }

    /// Where `offset` in the text before the edit is after it; see
    /// [`translate`].
    pub fn translate(&self, offset: usize, bias: Bias) -> usize {
        translate(offset, &self.old, self.new_len(), bias)
    }

    /// Where `range` in the text before the edit is after it; see
    /// [`translate_range`].
    pub fn translate_range(&self, range: Range<usize>) -> Range<usize> {
        translate_range(range, &self.old, self.new_len())
    }
}

/// Where `offset` in the text before an edit is after it, the edit having
/// replaced the bytes `old` with `new_len` bytes. An offset inside the
/// replaced bytes lands at the start of the new text with [`Bias::Left`], or
/// after it with [`Bias::Right`]; so does an offset at the start of the
/// replaced bytes with `Bias::Right`, which is how an insertion at an anchor
/// pushes the anchor along.
pub(crate) fn translate(offset: usize, old: &Range<usize>, new_len: usize, bias: Bias) -> usize {
    if offset < old.start || (offset == old.start && bias == Bias::Left) {
        offset
    } else if offset >= old.end {
        offset - old.len() + new_len
    } else {
        match bias {
            Bias::Left => old.start,
            Bias::Right => old.start + new_len,
        }
    }
}

/// Where `range` in the text before an edit is after it. The start keeps to
/// the left and the end to the right, so a range grows with typing at either
/// edge and shrinks only when its bytes are removed.
pub(crate) fn translate_range(
    range: Range<usize>,
    old: &Range<usize>,
    new_len: usize,
) -> Range<usize> {
    let start = translate(range.start, old, new_len, Bias::Left);
    let end = translate(range.end, old, new_len, Bias::Right);
    start..end.max(start)
}

/// The most edits the log keeps.
const MAX_EDITS: usize = 256;
/// The most bytes of new text the log keeps.
const MAX_BYTES: usize = 1 << 20;

/// The version of the text, counted up by every edit and every replacement
/// no edit describes, and the last edits made to it, each taking it from one
/// version to the next. What was computed against a version the log no
/// longer reaches back to has to be computed again.
#[derive(Debug, Default)]
pub(crate) struct EditLog {
    version: u64,
    edits: VecDeque<Edit>,
    /// The bytes of new text kept, in all.
    bytes: usize,
}

impl EditLog {
    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    /// Records the replacement of the bytes `old` with `new_text`, making
    /// the next version, forgetting the oldest edits past the log's limits.
    /// An edit too large for the log on its own empties it, and the log
    /// starts over at the version the edit made.
    pub(crate) fn push(&mut self, old: Range<usize>, new_text: &str) {
        self.version = self.version.wrapping_add(1);
        if new_text.len() > MAX_BYTES {
            self.edits.clear();
            self.bytes = 0;
            return;
        }
        self.bytes += new_text.len();
        self.edits.push_back(Edit::new(old, new_text));
        while (self.edits.len() > MAX_EDITS || self.bytes > MAX_BYTES)
            && let Some(oldest) = self.edits.pop_front()
        {
            self.bytes -= oldest.new_len();
        }
    }

    /// Forgets every edit: the next version came about in a way no edit
    /// describes.
    pub(crate) fn reset(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.edits.clear();
        self.bytes = 0;
    }

    /// The edits made since `version`, oldest first, or `None` if `version`
    /// is older than the log reaches back to.
    pub(crate) fn since(&self, version: u64) -> Option<impl Iterator<Item = &Edit>> {
        let behind = usize::try_from(self.version.checked_sub(version)?).ok()?;
        let skip = self.edits.len().checked_sub(behind)?;
        Some(self.edits.iter().skip(skip))
    }

    /// Where `range` in the text at `version` is now, or `None` if the log
    /// doesn't reach back to `version`.
    pub(crate) fn translate_range(
        &self,
        range: Range<usize>,
        version: u64,
    ) -> Option<Range<usize>> {
        let edits = self.since(version)?;
        Some(edits.fold(range, |range, edit| edit.translate_range(range)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offset_before_the_edit_stays_and_one_after_it_shifts() {
        let insert = Edit::new(5..5, "abc");
        assert_eq!(insert.translate(2, Bias::Left), 2);
        assert_eq!(insert.translate(2, Bias::Right), 2);
        assert_eq!(insert.translate(9, Bias::Left), 12);
        let delete = Edit::new(5..8, "");
        assert_eq!(delete.translate(9, Bias::Right), 6);
        assert_eq!(delete.translate(8, Bias::Left), 5);
        let replace = Edit::new(5..8, "xy");
        assert_eq!(replace.translate(8, Bias::Left), 7);
        assert_eq!(replace.translate(20, Bias::Right), 19);
    }

    #[test]
    fn an_offset_at_or_inside_the_edit_follows_its_bias() {
        let insert = Edit::new(5..5, "abc");
        assert_eq!(insert.translate(5, Bias::Left), 5);
        assert_eq!(insert.translate(5, Bias::Right), 8);
        let replace = Edit::new(5..8, "xy");
        assert_eq!(replace.translate(5, Bias::Left), 5);
        assert_eq!(replace.translate(5, Bias::Right), 7);
        assert_eq!(replace.translate(6, Bias::Left), 5);
        assert_eq!(replace.translate(6, Bias::Right), 7);
        let delete = Edit::new(5..8, "");
        assert_eq!(delete.translate(6, Bias::Left), 5);
        assert_eq!(delete.translate(6, Bias::Right), 5);
    }

    #[test]
    fn a_range_grows_with_typing_at_its_edges_and_shrinks_when_its_bytes_go() {
        assert_eq!(Edit::new(3..3, "ab").translate_range(3..6), 3..8);
        assert_eq!(Edit::new(6..6, "ab").translate_range(3..6), 3..8);
        assert_eq!(Edit::new(4..5, "").translate_range(3..6), 3..5);
        assert_eq!(Edit::new(0..10, "").translate_range(3..6), 0..0);
        assert_eq!(Edit::new(2..9, "x").translate_range(3..6), 2..3);
        assert_eq!(Edit::new(5..5, "ab").translate_range(5..5), 5..7);
    }

    #[test]
    fn the_log_gives_the_edits_since_a_version_it_still_holds() {
        let mut log = EditLog::default();
        for _ in 0..10 {
            log.reset();
        }
        log.push(0..0, "a");
        log.push(1..1, "b");
        log.push(0..2, "c");
        assert_eq!(log.version(), 13);
        let since = |version| {
            log.since(version)
                .map(|edits| edits.cloned().collect::<Vec<_>>())
        };
        assert_eq!(since(9), None);
        assert_eq!(since(10).map(|edits| edits.len()), Some(3));
        assert_eq!(since(12), Some(vec![Edit::new(0..2, "c")]));
        assert_eq!(since(13), Some(vec![]));
        assert_eq!(since(14), None);
        assert_eq!(log.translate_range(1..1, 10), Some(2..2));
        assert_eq!(log.translate_range(1..1, 13), Some(1..1));
        assert_eq!(log.translate_range(1..1, 9), None);
    }

    #[test]
    fn the_log_forgets_its_oldest_edits_past_its_limits() {
        let mut log = EditLog::default();
        for at in 0..MAX_EDITS + 2 {
            log.push(at..at, "x");
        }
        assert!(log.since(1).is_none());
        assert_eq!(log.since(2).map(Iterator::count), Some(MAX_EDITS));
        let mut log = EditLog::default();
        log.push(0..0, &"a".repeat(MAX_BYTES));
        log.push(0..0, "b");
        assert!(log.since(0).is_none());
        assert_eq!(log.since(1).map(Iterator::count), Some(1));
        log.reset();
        assert!(log.since(2).is_none());
        assert_eq!(log.since(3).map(Iterator::count), Some(0));
    }

    #[test]
    fn an_edit_too_large_for_the_log_starts_it_over() {
        let mut log = EditLog::default();
        log.push(0..0, "a");
        log.push(1..1, &"b".repeat(MAX_BYTES + 1));
        assert_eq!(log.since(1).map(Iterator::count), None);
        assert_eq!(log.since(2).map(Iterator::count), Some(0));
        log.push(0..1, "");
        assert_eq!(log.translate_range(1..3, 2), Some(0..2));
    }
}

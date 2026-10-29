//! The best of a search's hits, in order, and the characters of each that matched.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ops::Range;

use nucleo_matcher::Utf32Str;
use nucleo_matcher::pattern::Pattern;

use super::pool::{Matching, Pool};
use super::scan::Hit;

/// What being among the files opened lately adds to a match's score: about one character
/// matched at a word's start, so it lifts a file above its equals but not over a clearly
/// better match.
const RECENT: u32 = 32;

/// Hits a thread ranks at least, so a search with few doesn't share them out.
const PER_THREAD: usize = 16_384;

/// A hit as it's ranked: by score, then how lately it was opened, then the shorter path,
/// then the file tree's order.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ranked {
    pub hit: Hit,
    /// Its place among the files opened lately, the latest 0; `u32::MAX` if it isn't.
    pub recency: u32,
}

/// The `most` best of `hits`, best first, `recent` holding where the files opened lately
/// are, the latest first. The hits are shared among the pool's threads, each picking the
/// best of its share, and the best of theirs are picked here.
pub(super) fn best(hits: &[Hit], recent: &[(u32, u32)], most: usize, pool: &Pool) -> Vec<Ranked> {
    if most == 0 {
        return Vec::new();
    }
    let shares = pool.threads().min(hits.len() / PER_THREAD).max(1);
    let share = hits.len().div_ceil(shares).max(1);
    let mut ranked = pool
        .each(hits.chunks(share).collect(), |share| best_of(share, most))
        .concat();
    // A recent file can climb into the best, from wherever it was among the hits.
    for (recency, spot) in recent.iter().enumerate() {
        let Ok(at) = hits.binary_search_by(|hit| hit.spot().cmp(spot)) else {
            continue;
        };
        let lifted = Ranked {
            hit: Hit {
                score: hits[at].score + RECENT,
                ..hits[at]
            },
            recency: recency as u32,
        };
        match ranked.iter_mut().find(|ranked| ranked.hit.spot() == *spot) {
            Some(ranked) => *ranked = lifted,
            None => ranked.push(lifted),
        }
    }
    ranked.sort_unstable_by(compare);
    ranked.truncate(most);
    ranked
}

/// The `most` best of `hits`, in no order, none of them recent yet: kept in a heap with the
/// worst of them on top, whose place any better hit takes.
fn best_of(hits: &[Hit], most: usize) -> Vec<Ranked> {
    let mut kept = BinaryHeap::with_capacity(most.min(hits.len()));
    for &hit in hits {
        let ranked = Ranked {
            hit,
            recency: u32::MAX,
        };
        if kept.len() < most {
            kept.push(Worst(ranked));
        } else if let Some(mut worst) = kept.peek_mut()
            && compare(&ranked, &worst.0) == Ordering::Less
        {
            *worst = Worst(ranked);
        }
    }
    kept.into_iter().map(|worst| worst.0).collect()
}

/// The better of two first.
fn compare(a: &Ranked, b: &Ranked) -> Ordering {
    (b.hit.score.cmp(&a.hit.score))
        .then(a.recency.cmp(&b.recency))
        .then(a.hit.length.cmp(&b.hit.length))
        .then(a.hit.spot().cmp(&b.hit.spot()))
}

/// A hit ranked, the worse the greater, so a heap of them has the worst on top.
struct Worst(Ranked);

impl Ord for Worst {
    fn cmp(&self, other: &Self) -> Ordering {
        compare(&self.0, &other.0)
    }
}

impl PartialOrd for Worst {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Worst {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Worst {}

/// The bytes of `path` that `pattern` matched: within the name, which starts at `name`, if
/// that's where it matched (`named`), or else across the whole path.
pub(super) fn highlights(
    pattern: &Pattern,
    path: &str,
    (name, named): (usize, bool),
    matching: &mut Matching,
) -> Vec<Range<usize>> {
    let Matching {
        matcher,
        chars,
        indices,
    } = matching;
    let offset = if named { name } else { 0 };
    let searched = path.get(offset..).unwrap_or(path);
    indices.clear();
    pattern.indices(Utf32Str::new(searched, chars), matcher, indices);
    indices.sort_unstable();
    indices.dedup();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut wanted = indices.iter().copied().peekable();
    for (nth, (at, char)) in searched.char_indices().enumerate() {
        if wanted.peek() != Some(&(nth as u32)) {
            continue;
        }
        wanted.next();
        let range = offset + at..offset + at + char.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == range.start => last.end = range.end,
            _ => ranges.push(range),
        }
    }
    ranges
}

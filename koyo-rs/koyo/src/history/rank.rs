//! How the history ranks what it holds, as Firefox's address bar does (`docs/new-tab.md` 4):
//! frecency, Firefox 147's exponential decay over an entry's latest uses; what was typed before an
//! entry was picked, which brings it first when typed again; every word typed found in an entry's
//! title or address, at the start of a word before anywhere; and a host completed in the field as
//! it's typed. Entries used in the checkout at hand, or its project, rank higher.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::address::{display_url, host, strip_scheme};
use super::{Entry, History, How, Kind, Learned, Place, SAMPLED};

/// A day, in seconds.
const DAY: f64 = 24. * 60. * 60.;
/// How many days a use takes to count half as much (`places.frecency.pages.halfLifeDays`).
const HALF_LIFE_DAYS: f64 = 30.;
/// The weight of a page with the bare minimum a top site needs (`pageFrecencyThreshold(30, 7)`:
/// seven links followed thirty days ago), which the empty launcher's pages reach.
const TOP_SITE_LEAST: f64 = 87.5;
/// How many days back a typed visit lets its host complete what's typed.
const AUTOFILL_DAYS: f64 = 90.;
/// How many results matched at the start of a word are enough: with fewer, words matched
/// anywhere count too (`UrlbarProviderPlaces`, `maxResults` × 1.5).
const ENOUGH_AT_WORDS: usize = 15;
/// How many results picked before after typing what's typed lead the rest.
const MOST_LEARNED: usize = 3;
/// How much more an entry used in the checkout at hand, or in its project, ranks.
const SAME_WORKTREE: f64 = 3.;
const SAME_PROJECT: f64 = 2.;
/// How much more words that start with what's typed rank than words that only hold it, as
/// Firefox ranks its form history's prefix matches.
const PREFIX: f64 = 5.;
/// What a pick learned keeps of itself each day, as Firefox's input history fades daily.
const LEARNED_FADE: f32 = 0.975;
/// Learned picks faded below this, ninety days unpicked, are no longer offered.
const LEARNED_LEAST: f32 = 0.1;

/// What a use weighs (`places.frecency.pages.*Weight`): asking for something by name, twice a
/// link followed.
fn weight(how: How) -> f64 {
    match how {
        How::Typed => 100.,
        How::Followed => 50.,
    }
}

/// How often and how lately `entry` was used, now: Firefox's frecency, each of its latest uses
/// weighed and halved every thirty days since, averaged, times how many uses it had in all.
pub(crate) fn strength(entry: &Entry, now: i64) -> f64 {
    let decay = std::f64::consts::LN_2 / HALF_LIFE_DAYS;
    let (sum, n) = (entry.uses.iter().rev().take(SAMPLED)).fold((0., 0.), |(sum, n), used| {
        (
            sum + weight(used.how) * (-decay * days(now - used.at)).exp(),
            n + 1.,
        )
    });
    if n == 0. {
        return 0.;
    }
    sum / n * f64::from(entry.count).max(n)
}

fn days(seconds: i64) -> f64 {
    seconds.max(0) as f64 / DAY
}

/// What `learned` counts for at `now`, faded by the days since it was last picked.
pub(crate) fn faded(learned: &Learned, now: i64) -> f32 {
    let days = ((now - learned.at).max(0) as f64 / DAY) as i32;
    learned.uses * LEARNED_FADE.powi(days)
}

/// How much more `entry` ranks for being used in `place`.
fn nearness(entry: &Entry, place: &Place) -> f64 {
    if entry.worktrees.contains(&place.worktree) {
        SAME_WORKTREE
    } else if entry.projects.contains(&place.project) {
        SAME_PROJECT
    } else {
        1.
    }
}

/// An entry the history found, as the launcher shows it: what it is, the text it shows and the
/// bytes of each that matched.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub kind: Kind,
    /// The page's address, or the words.
    pub text: String,
    /// A page's title, or the words.
    pub title: String,
    pub title_matched: Vec<Range<usize>>,
    /// A page's address as shown (`display_url`); empty for words.
    pub detail: String,
    pub detail_matched: Vec<Range<usize>>,
    /// Picked before after typing what's typed now.
    pub learned: bool,
}

impl History {
    /// The entries of `kind` for `query`, best first, at most `most`. Typed, they hold every word
    /// of it: up to three picked before after typing it first, the rest by frecency. With nothing
    /// typed, a browser's top sites or the latest words; either way those of `place` first.
    pub fn find(
        &self,
        kind: Kind,
        query: &str,
        place: &Place,
        most: usize,
        now: i64,
    ) -> Vec<Found> {
        let tokens: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if tokens.is_empty() {
            return match kind {
                Kind::Page => self.top_sites(place, most, now),
                _ => self.latest(kind, place, most),
            };
        }
        let mut found = self.learned_for(kind, query, &tokens, MOST_LEARNED.min(most), now);
        let typed = query.trim().to_lowercase();
        // Each entry holding every word, and whether each starts a word in it.
        let held: Vec<(&Entry, bool)> = (self.of(kind))
            .filter(|entry| !found.iter().any(|learned| learned.text == entry.text))
            .filter_map(|entry| Some((entry, holds(entry, &tokens)?)))
            .collect();
        let enough_at_words =
            held.iter().filter(|(_, at_words)| *at_words).count() >= ENOUGH_AT_WORDS;
        let mut ranked: Vec<(f64, &Entry)> = (held.into_iter())
            .filter(|(_, at_words)| *at_words || !enough_at_words)
            .map(|(entry, _)| {
                let starts = kind != Kind::Page && starts_with_ignoring_case(&entry.text, &typed);
                let prefix = if starts { PREFIX } else { 1. };
                (
                    strength(entry, now) * nearness(entry, place) * prefix,
                    entry,
                )
            })
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let room = most.saturating_sub(found.len());
        found.extend((ranked.into_iter().take(room)).map(|(_, entry)| shown(entry, &tokens)));
        found
    }

    /// Pages with nothing typed, as Firefox's top sites: the most frecent page of each host, the
    /// host weighed by all its pages, those that reach a top site's least.
    fn top_sites(&self, place: &Place, most: usize, now: i64) -> Vec<Found> {
        // Each host: the strength of all its pages, and its strongest page and how strong.
        let mut hosts: HashMap<String, (f64, f64, &Entry)> = HashMap::new();
        for entry in self.of(Kind::Page) {
            let page = strength(entry, now) * nearness(entry, place);
            let host = host(&entry.text).unwrap_or_else(|| entry.text.clone());
            let known = hosts.entry(host).or_insert((0., f64::MIN, entry));
            known.0 += page;
            if page > known.1 {
                (known.1, known.2) = (page, entry);
            }
        }
        let mut hosts: Vec<(f64, &Entry)> = (hosts.into_values())
            .filter(|(weight, ..)| *weight >= TOP_SITE_LEAST)
            .map(|(weight, _, entry)| (weight, entry))
            .collect();
        // Ties go to the page used last, the store keeping it last.
        hosts.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| b.1.last_used().cmp(&a.1.last_used()))
        });
        (hosts.into_iter().take(most))
            .map(|(_, entry)| shown(entry, &[]))
            .collect()
    }

    /// Words with nothing typed, the latest first, those used in `place`'s project ahead.
    fn latest(&self, kind: Kind, place: &Place, most: usize) -> Vec<Found> {
        // The store keeps the entry used last at its end: ties go to it.
        let mut entries: Vec<&Entry> = self.of(kind).rev().collect();
        entries.sort_by_key(|entry| {
            let here = nearness(entry, place) > 1.;
            std::cmp::Reverse((here, entry.last_used()))
        });
        (entries.into_iter().take(most))
            .map(|entry| shown(entry, &[]))
            .collect()
    }

    /// The entries of `kind` picked before after typing something `query` starts, the most picked
    /// first, and those picked after typing it exactly counting twice; at most `most`.
    fn learned_for(
        &self,
        kind: Kind,
        query: &str,
        tokens: &[String],
        most: usize,
        now: i64,
    ) -> Vec<Found> {
        let typed = query.trim().to_lowercase();
        let mut learned: Vec<(f32, &Learned)> = (self.learned.iter())
            .filter(|learned| learned.kind == kind && learned.input.starts_with(&typed))
            .map(|learned| {
                let exact = if learned.input == typed { 2. } else { 1. };
                (faded(learned, now) * exact, learned)
            })
            .filter(|(score, _)| *score >= LEARNED_LEAST)
            .collect();
        learned.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut found: Vec<Found> = Vec::new();
        for (_, learned) in learned {
            if found.len() == most {
                break;
            }
            if found.iter().any(|found| found.text == learned.text) {
                continue;
            }
            if let Some(entry) = self.of(kind).find(|entry| entry.text == learned.text) {
                found.push(Found {
                    learned: true,
                    ..shown(entry, tokens)
                });
            }
        }
        found
    }

    /// The host to complete `typed` with (`Origins::complete`), as of `now`.
    #[cfg(test)]
    pub fn autofill(&self, typed: &str, now: i64) -> Option<String> {
        self.origins(now).complete(typed)
    }

    /// The hosts that may complete what's typed, as Firefox autofills origins: those typed in the
    /// last ninety days, each as strong as its pages on average, times the days it was typed; and
    /// the median of them, which a host completing what's typed has to reach. What's typed doesn't
    /// change them, so the launcher works them out once.
    pub fn origins(&self, now: i64) -> Origins {
        // Each host: the strength of its pages, how many, and the days it was typed on.
        let mut hosts: HashMap<String, (f64, usize, HashSet<i64>)> = HashMap::new();
        for entry in self.of(Kind::Page) {
            let Some(host) = host(&entry.text) else {
                continue;
            };
            let known = hosts.entry(host).or_default();
            known.0 += strength(entry, now);
            known.1 += 1;
            let typed_days = (entry.uses.iter())
                .filter(|used| used.how == How::Typed && days(now - used.at) <= AUTOFILL_DAYS)
                .map(|used| used.at / DAY as i64);
            known.2.extend(typed_days);
        }
        let scored: Vec<(String, f64)> = (hosts.into_iter())
            .filter(|(_, (.., typed_days))| !typed_days.is_empty())
            .map(|(host, (sum, pages, typed_days))| {
                (host, sum / pages as f64 * typed_days.len() as f64)
            })
            .collect();
        let threshold = median(scored.iter().map(|(_, score)| *score).collect());
        Origins { scored, threshold }
    }
}

/// The hosts that may complete what's typed in a browser launcher (`History::origins`).
#[derive(Debug, Default)]
pub struct Origins {
    scored: Vec<(String, f64)>,
    threshold: f64,
}

impl Origins {
    /// The host to complete `typed` with: of those whose name starts with it, the strongest, if
    /// it's at least as strong as the median host. `None` while what's typed has spaces or a
    /// path, or nothing completes it.
    pub fn complete(&self, typed: &str) -> Option<String> {
        let typed = typed.to_lowercase();
        if typed.is_empty() || typed.contains(char::is_whitespace) || typed.contains('/') {
            return None;
        }
        (self.scored.iter())
            .filter(|(host, score)| host.starts_with(&typed) && *score >= self.threshold)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(host, _)| host.clone())
    }
}

/// The greatest of the lower half of `scores`, as Firefox's origin threshold.
fn median(mut scores: Vec<f64>) -> f64 {
    scores.sort_by(f64::total_cmp);
    let lower = scores.len().div_ceil(2);
    lower
        .checked_sub(1)
        .and_then(|at| scores.get(at))
        .copied()
        .unwrap_or(0.)
}

/// Whether every one of `tokens` is in `entry`'s title or address, or its words: if so, whether
/// each starts a word there.
pub(super) fn holds(entry: &Entry, tokens: &[String]) -> Option<bool> {
    let address = if entry.kind == Kind::Page {
        strip_scheme(&entry.text)
    } else {
        ""
    };
    let title = if entry.kind == Kind::Page {
        entry.title.as_str()
    } else {
        entry.text.as_str()
    };
    let mut at_words = true;
    for token in tokens {
        let (in_title, title_word) = hits(title, token);
        let (in_address, address_word) = if title_word {
            (true, true)
        } else {
            hits(address, token)
        };
        if !(in_title || in_address) {
            return None;
        }
        at_words &= title_word || address_word;
    }
    Some(at_words)
}

/// Whether `token`, lowercased, is in `text`, ignoring case, and whether it starts a word there.
/// ASCII text, as most titles and addresses are, is searched as it is, nothing allocated.
fn hits(text: &str, token: &str) -> (bool, bool) {
    if !(text.is_ascii() && token.is_ascii()) {
        let found = find_all(text, token);
        return (
            !found.is_empty(),
            found.iter().any(|range| at_word(text, range.start)),
        );
    }
    let (bytes, wanted) = (text.as_bytes(), token.as_bytes());
    if wanted.is_empty() || wanted.len() > bytes.len() {
        return (false, false);
    }
    let mut any = false;
    for at in 0..=bytes.len() - wanted.len() {
        if bytes[at..at + wanted.len()].eq_ignore_ascii_case(wanted) {
            if at_word(text, at) {
                return (true, true);
            }
            any = true;
        }
    }
    (any, false)
}

/// Whether `text` starts with `prefix`, lowercased, ignoring case.
fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
    if text.is_ascii() {
        return text
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix));
    }
    text.to_lowercase().starts_with(prefix)
}

/// Whether byte `at` of `text` starts a word, as Firefox tells: anything but a lowercase letter
/// after a letter, so `goo` starts in `www.google.com` and `hub` in `GitHub`.
fn at_word(text: &str, at: usize) -> bool {
    let Some(c) = text[at..].chars().next() else {
        return false;
    };
    let before = text[..at].chars().next_back();
    !(c.is_lowercase() && before.is_some_and(char::is_alphabetic))
}

/// `entry` as the launcher shows it, with the bytes of each of `tokens` in it: a page by its
/// title, or its address where it has none, its address after it; words on one line.
fn shown(entry: &Entry, tokens: &[String]) -> Found {
    let (title, detail) = match entry.kind {
        Kind::Page => {
            let address = display_url(&entry.text);
            let title = entry.title.trim();
            if title.is_empty() {
                (address, String::new())
            } else {
                (title.to_owned(), address)
            }
        }
        _ => (
            entry.text.split_whitespace().collect::<Vec<_>>().join(" "),
            String::new(),
        ),
    };
    let marked = |text: &str| {
        let mut ranges: Vec<Range<usize>> = tokens
            .iter()
            .flat_map(|token| find_all(text, token))
            .collect();
        ranges.sort_by_key(|range| range.start);
        ranges
    };
    Found {
        kind: entry.kind,
        text: entry.text.clone(),
        title_matched: marked(&title),
        detail_matched: marked(&detail),
        title,
        detail,
        learned: false,
    }
}

/// Every place `token`, lowercased, is in `text`, ignoring case, as byte ranges of `text`.
fn find_all(text: &str, token: &str) -> Vec<Range<usize>> {
    if token.is_empty() {
        return Vec::new();
    }
    let mut lower = String::with_capacity(text.len());
    // For each byte of `lower`, the character of `text` it comes from.
    let mut from: Vec<Range<usize>> = Vec::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        for l in c.to_lowercase() {
            from.extend(std::iter::repeat_n(at..at + c.len_utf8(), l.len_utf8()));
            lower.push(l);
        }
    }
    (lower.match_indices(token))
        .filter_map(|(at, matched)| {
            let first = from.get(at)?;
            let last = from.get(at + matched.len() - 1)?;
            Some(first.start..last.end)
        })
        .collect()
}

/// Whether every word of `query` is in one of `fields`, ignoring case: if so, for each field, the
/// bytes of it the words matched.
pub fn match_words(fields: &[&str], query: &str) -> Option<Vec<Vec<Range<usize>>>> {
    let tokens: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut marked: Vec<Vec<Range<usize>>> = vec![Vec::new(); fields.len()];
    for token in &tokens {
        let mut any = false;
        for (field, marks) in fields.iter().zip(&mut marked) {
            let found = find_all(field, token);
            any |= !found.is_empty();
            marks.extend(found);
        }
        if !any {
            return None;
        }
    }
    for marks in &mut marked {
        marks.sort_by_key(|range| range.start);
    }
    Some(marked)
}

use crate::async_util::{CancelOnDrop, cancelled};
use crate::input::InputModeKind;
use aho_corasick::AhoCorasick;
use gpui::{Context, Window};
use ropey::Rope;
use std::{
    iter,
    ops::{ControlFlow, Range},
    sync::atomic::AtomicBool,
};
use sum_tree::{Bias, Dimensions, SumTree};

use super::{
    InputBaseState, Replace, RopeExt as _, Search, movement::MoveDirection, state::ScrollPadding,
};

/// Bytes of the text searched at a time, copied out of the rope.
const SEARCH_WINDOW: usize = 64 * 1024;
/// Bytes a new query searches on the main thread. Past them, the search
/// shows the matches found from the view on, and searches the whole text in
/// the background.
const SYNC_SEARCH_BYTES: usize = 1024 * 1024;
/// Bytes a background search goes through between checks for whether a newer
/// search superseded it.
const FULL_SEARCH_CHUNK: usize = 1024 * 1024;

/// Stateful, presentation-independent search engine used by text inputs.
#[derive(Debug, Clone)]
pub struct SearchMatcher {
    text: Rope,
    pub query: Option<AhoCorasick>,
    matches: Matches,
    current_match_ix: usize,
    replacing: bool,
    /// Whether every change to the input's text since `text` reaches the
    /// matcher through [`SearchMatcher::edited`], so `text` is the input's.
    tracking: bool,
    /// The search of the whole text under way in the background, see
    /// [`SearchMatcher::update_query_from`].
    pending: Option<PendingSearch>,
}

/// A search of the whole text under way in the background: where it was
/// anchored, and one edit spanning every edit made since it took the text,
/// to search again around once its matches land.
#[derive(Debug, Clone)]
struct PendingSearch {
    anchor: usize,
    edit: Option<(Range<usize>, usize)>,
}

/// A search of a whole text, for a background thread.
pub(crate) struct FullSearch {
    query: AhoCorasick,
    text: Rope,
}

/// The matches a [`FullSearch`] found.
pub(crate) struct FullSearchMatches(Matches);

impl FullSearch {
    /// Searches the whole text, or stops once `cancel` is set.
    pub(crate) fn run(self, cancel: &AtomicBool) -> Option<FullSearchMatches> {
        let mut ranges = Vec::new();
        let mut window = Vec::new();
        let mut pos = 0;
        while pos < self.text.len() {
            if cancelled(cancel) {
                return None;
            }
            let from = pos;
            pos += FULL_SEARCH_CHUNK;
            for found in find_all(&self.query, &self.text, from, pos, &mut window) {
                pos = pos.max(found.end);
                ranges.push(found);
            }
        }
        Some(FullSearchMatches(Matches::from_ranges(ranges)))
    }
}

/// Matches in text order, without overlaps. Each is kept relative to the one
/// before it, so an edit changes only the matches around it however many
/// follow it, and a copy shares them.
#[derive(Debug, Clone, Default)]
struct Matches(SumTree<Match>);

/// A match: how far it starts after the one before ends, or after the start
/// of the text, and its length.
#[derive(Debug, Clone)]
struct Match {
    gap: usize,
    len: usize,
}

#[derive(Debug, Clone, Default)]
struct MatchSummary {
    count: usize,
    /// Where the last match ends.
    end: usize,
}

impl sum_tree::ContextLessSummary for MatchSummary {
    fn zero() -> Self {
        Self::default()
    }

    fn add_summary(&mut self, other: &Self) {
        self.count += other.count;
        self.end += other.end;
    }
}

impl sum_tree::Item for Match {
    type Summary = MatchSummary;

    fn summary(&self, _: ()) -> MatchSummary {
        MatchSummary {
            count: 1,
            end: self.gap + self.len,
        }
    }
}

/// Cursor dimension counting matches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct MatchCount(usize);

impl<'a> sum_tree::Dimension<'a, MatchSummary> for MatchCount {
    fn zero(_: ()) -> Self {
        Self(0)
    }

    fn add_summary(&mut self, summary: &'a MatchSummary, _: ()) {
        self.0 += summary.count;
    }
}

/// Cursor dimension: where the matches so far end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct MatchEnd(usize);

impl<'a> sum_tree::Dimension<'a, MatchSummary> for MatchEnd {
    fn zero(_: ()) -> Self {
        Self(0)
    }

    fn add_summary(&mut self, summary: &'a MatchSummary, _: ()) {
        self.0 += summary.end;
    }
}

impl Matches {
    /// Matches from `ranges`, in text order and without overlaps.
    fn from_ranges(ranges: impl IntoIterator<Item = Range<usize>>) -> Self {
        let mut end = 0;
        let matches = ranges.into_iter().map(|found| {
            let gap = found.start - end;
            end = found.end;
            Match {
                gap,
                len: found.len(),
            }
        });
        Self(SumTree::from_iter(matches, ()))
    }

    fn len(&self) -> usize {
        self.0.summary().count
    }

    fn get(&self, ix: usize) -> Option<Range<usize>> {
        let mut cursor = self.0.cursor::<Dimensions<MatchCount, MatchEnd>>(());
        cursor.seek(&MatchCount(ix), Bias::Right);
        let found = cursor.item()?;
        let start = cursor.start().1.0 + found.gap;
        Some(start..start + found.len)
    }

    /// How many matches end at `offset` or before.
    fn ending_by(&self, offset: usize) -> usize {
        let mut cursor = self.0.cursor::<Dimensions<MatchEnd, MatchCount>>(());
        cursor.seek(&MatchEnd(offset), Bias::Right);
        cursor.start().1.0
    }

    /// How many matches start before `offset`.
    fn starting_before(&self, offset: usize) -> usize {
        let ix = self.ending_by(offset);
        ix + usize::from(self.get(ix).is_some_and(|found| found.start < offset))
    }

    /// The matches that reach into `range`, with their indices.
    fn within(&self, range: Range<usize>) -> impl Iterator<Item = (usize, Range<usize>)> + '_ {
        let mut cursor = self.0.cursor::<Dimensions<MatchEnd, MatchCount>>(());
        cursor.seek(&MatchEnd(range.start), Bias::Right);
        iter::from_fn(move || {
            let found = cursor.item()?;
            let Dimensions(MatchEnd(end), MatchCount(ix), ()) = *cursor.start();
            let start = end + found.gap;
            if start >= range.end {
                return None;
            }
            cursor.next();
            Some((ix, start..start + found.len))
        })
    }

    fn iter(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let mut end = 0;
        self.0.iter().map(move |found| {
            let start = end + found.gap;
            end = start + found.len;
            start..end
        })
    }
}

/// One search over an input: the query, how the built-in panel shows it, and
/// its matches. Read it through [`InputBaseState::search_session`]; it is
/// written only through the input state's search methods, and it grows, so
/// build it with `Default` and do not destructure it exhaustively.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SearchSession {
    /// The built-in search panel is showing.
    pub open: bool,
    pub replace_mode: bool,
    pub case_insensitive: bool,
    pub query: String,
    pub replacement: String,
    pub anchor_offset: Option<usize>,
    pub matcher: SearchMatcher,
    /// A search is in progress and its matches are highlighted: the panel is
    /// open, or a query was set without it and not closed since.
    active: bool,
}

impl Default for SearchSession {
    fn default() -> Self {
        Self {
            open: false,
            active: false,
            replace_mode: false,
            case_insensitive: true,
            query: String::new(),
            replacement: String::new(),
            anchor_offset: None,
            matcher: SearchMatcher::new(),
        }
    }
}

impl SearchSession {
    pub(crate) fn open(&mut self, replace_mode: bool, replaceable: bool) {
        self.open = true;
        self.active = true;
        self.replace_mode = replace_mode && replaceable;
    }

    /// Start a search without the built-in panel. A custom search UI drives
    /// the session through [`InputBaseState::set_search_query`], and the
    /// editor highlights the matches the same way it does for the panel.
    pub(crate) fn activate(&mut self) {
        self.active = true;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.active = false;
    }

    /// Whether a search is in progress: the built-in panel is open, or a
    /// query was set without it and [`InputBaseState::close_search`] has not
    /// run since. Matches are highlighted while this holds.
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub(crate) fn update_query(&mut self, query: impl Into<String>, case_insensitive: bool) {
        if self.set_query(query.into(), case_insensitive) {
            self.matcher.update_query(&self.query, case_insensitive);
        }
    }

    /// [`Self::update_query`] for a new query, searching at most
    /// [`SYNC_SEARCH_BYTES`] at once, from `anchor`: a search of the whole
    /// text to run in the background is returned when that isn't all of it.
    fn update_query_from(
        &mut self,
        query: String,
        case_insensitive: bool,
        anchor: usize,
    ) -> Option<FullSearch> {
        self.set_query(query, case_insensitive);
        self.matcher
            .update_query_from(&self.query, case_insensitive, anchor)
    }

    /// Returns whether the query or the case mode changed.
    fn set_query(&mut self, query: String, case_insensitive: bool) -> bool {
        if self.query == query && self.case_insensitive == case_insensitive {
            return false;
        }
        self.query = query;
        self.case_insensitive = case_insensitive;
        true
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    /// Open the search session, or re-invoke it if it is already open.
    ///
    /// This is not idempotent: every call advances
    /// [`InputBaseState::search_activation_revision`], and the presentation
    /// layer answers that by re-focusing the search field and selecting its
    /// contents, the same as pressing the shortcut a second time. Call it from
    /// an action or another user gesture, never from a render pass or an
    /// observer that runs every frame — that would re-select the field under
    /// the user on every frame and make it impossible to type.
    pub fn open_search(&mut self, replace_mode: bool, cx: &mut Context<Self>) {
        if !self.searchable {
            return;
        }
        self.search_activation_revision = self.search_activation_revision.wrapping_add(1);
        self.search_session
            .open(replace_mode, self.is_replaceable());
        let selected = self.selected_text().to_string();
        let query = if selected.is_empty() {
            self.search_session.query.clone()
        } else {
            selected
        };
        let query_changed = query != self.search_session.query;
        // A retained query resumes its previous occurrence. Only a new query
        // is anchored to the current viewport.
        self.search_session.anchor_offset = if query_changed {
            self.last_layout
                .as_ref()
                .map(|layout| layout.visible_range_offset.start)
        } else {
            None
        };
        let case_insensitive = self.search_session.case_insensitive;
        self.search_for(query, case_insensitive, query_changed, cx);
        cx.notify();
    }

    /// Search for `query`: a changed one searches the text afresh, in the
    /// background past [`SYNC_SEARCH_BYTES`] from its anchor, and the same
    /// one catches up with the edits since.
    fn search_for(
        &mut self,
        query: String,
        case_insensitive: bool,
        query_changed: bool,
        cx: &mut Context<Self>,
    ) {
        if query_changed {
            self.search_session.matcher.take_text(&self.text);
            let anchor = self.search_session.anchor_offset.unwrap_or(0);
            match self
                .search_session
                .update_query_from(query, case_insensitive, anchor)
            {
                Some(full) => {
                    // Replacing the task stops the search a newer one supersedes.
                    let cancel = CancelOnDrop::new();
                    let flag = cancel.flag();
                    let search = cx
                        .background_executor()
                        .spawn(async move { full.run(&flag) });
                    self.search_task = Some(cx.spawn(async move |this, cx| {
                        let _cancel = cancel;
                        let Some(found) = search.await else {
                            return;
                        };
                        this.update(cx, |state, cx| {
                            if state.search_session.matcher.finish_full_search(found) {
                                cx.notify();
                            }
                        })
                        .ok();
                    }));
                }
                None => self.search_task = None,
            }
        } else {
            // The same query: the matches only catch up with the text.
            self.search_session.update_query(query, case_insensitive);
            self.sync_search_matcher();
        }
        if query_changed && let Some(anchor) = self.search_session.anchor_offset {
            self.search_session.matcher.update_cursor_by_offset(anchor);
        }
    }

    pub fn search_session(&self) -> &SearchSession {
        &self.search_session
    }

    /// A counter that advances every time [`InputBaseState::open_search`] runs,
    /// including while the session is already open.
    ///
    /// Re-invoking search leaves the session itself identical, so a presentation
    /// layer that decides what to rebuild by comparing session state cannot see
    /// the second request. Fold this into that comparison to notice it.
    pub fn search_activation_revision(&self) -> u64 {
        self.search_activation_revision
    }

    #[doc(hidden)]
    pub fn set_search_replace_mode(&mut self, replace_mode: bool, cx: &mut Context<Self>) {
        self.search_session.replace_mode = replace_mode && self.is_replaceable();
        cx.notify();
    }

    /// Returns true if the search panel can replace the matches.
    ///
    /// This is false when the input is not `replaceable`, or when it is
    /// `disabled` or `readonly`.
    pub fn is_replaceable(&self) -> bool {
        self.replaceable && self.is_editable()
    }

    /// Set the search query and highlight its matches.
    ///
    /// This is the entry point for a custom search UI: it needs neither
    /// `searchable` nor the built-in panel. Navigate the matches with
    /// [`InputBaseState::next_search_match`] and
    /// [`InputBaseState::previous_search_match`], read the count and the
    /// current index from [`InputBaseState::search_session`], and end the
    /// search with [`InputBaseState::close_search`].
    ///
    /// A changed query or case mode starts at the current viewport. Setting
    /// the same query and case mode again retains the current occurrence.
    pub fn set_search_query(
        &mut self,
        query: impl Into<String>,
        case_insensitive: bool,
        cx: &mut Context<Self>,
    ) {
        let query = query.into();
        let query_changed = query != self.search_session.query
            || case_insensitive != self.search_session.case_insensitive;
        self.search_session.anchor_offset = if query_changed {
            self.last_layout
                .as_ref()
                .map(|layout| layout.visible_range_offset.start)
        } else {
            None
        };
        self.search_session.activate();
        self.search_for(query, case_insensitive, query_changed, cx);
        cx.notify();
    }

    /// End the search: hide the built-in panel and the match highlights. The
    /// query is kept so the next [`InputBaseState::open_search`] resumes it.
    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        self.search_session.close();
        cx.notify();
    }

    pub fn next_search_match(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.sync_search_matcher();
        let range = self.search_session.matcher.next()?;
        // Match order does not describe viewport direction after a manual
        // scroll. Always allow search navigation to reveal the active match.
        self.scroll_to_with_padding(range.end, None, ScrollPadding::SurroundingLines, cx);
        Some(range)
    }

    pub fn previous_search_match(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.sync_search_matcher();
        let range = self.search_session.matcher.next_back()?;
        // Match order does not describe viewport direction after a manual
        // scroll. Always allow search navigation to reveal the active match.
        self.scroll_to_with_padding(range.start, None, ScrollPadding::SurroundingLines, cx);
        Some(range)
    }

    /// Replace the current match and move on to the next one. Returns whether
    /// there was a match to replace.
    pub fn replace_current_search_match(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_replaceable() {
            return false;
        }
        self.sync_search_matcher();
        let matcher = &mut self.search_session.matcher;
        let Some(range) = matcher.match_at(matcher.current_match_index()) else {
            return false;
        };
        let next = matcher.peek().unwrap_or_else(|| range.clone());
        let direction = matcher
            .has_next_without_wrap()
            .then_some(MoveDirection::Down);
        if direction.is_none() {
            matcher.set_current_match_index(0);
        }
        matcher.begin_replacement();
        let range_utf16 = self.range_to_utf16(&range);
        self.scroll_to(next.end, direction, cx);
        self.replace_text_in_range_silent(Some(range_utf16), replacement, window, cx);
        true
    }

    /// Replace every match. Returns how many were replaced.
    pub fn replace_all_search_matches(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        if !self.is_replaceable() {
            return 0;
        }
        self.sync_search_matcher();
        // Every match, not only those found before the background search
        // lands.
        self.search_session.matcher.complete();
        self.search_task = None;
        let ranges = self.search_session.matcher.matched_ranges();
        if ranges.is_empty() {
            return 0;
        }
        let mut text = self.text.clone();
        for range in ranges.iter().rev() {
            text.replace(range.clone(), replacement);
        }
        self.search_session.matcher.begin_replacement();
        let count = ranges.len();
        self.replace_text_in_range_silent(Some(0..self.text.len()), &text.to_string(), window, cx);
        self.scroll_to(0, Some(MoveDirection::Down), cx);
        count
    }

    /// Keep the matches in step with an edit. A closed search skips the scan:
    /// it copies and searches the whole document, and nothing reads the
    /// matches until the search is resumed or navigated, which sync first.
    pub(super) fn update_search(&mut self, _cx: &mut gpui::App) {
        if !self.search_session.is_active() {
            return;
        }
        self.sync_search_matcher();
    }

    /// Recompute the matches if the text changed since the last scan.
    fn sync_search_matcher(&mut self) {
        self.search_session.matcher.sync(&self.text);
    }

    /// Keep the matches in step with an edit of `range`, now `new_len` bytes
    /// of the text: an active search searches again around it, a closed one
    /// does nothing until it is resumed, and then searches everything again.
    pub(super) fn search_text_edited(&mut self, range: &Range<usize>, new_len: usize) {
        if self.search_session.is_active() {
            self.search_session
                .matcher
                .edited(&self.text, range.clone(), new_len);
        } else {
            self.search_text_replaced();
        }
    }

    /// The text changed in a way no edit describes: search it all again,
    /// and stop a background search, which can't catch up with it.
    pub(super) fn search_text_replaced(&mut self) {
        self.search_session.matcher.untrack();
        self.search_task = None;
    }

    /// An input that is not `searchable` leaves the shortcut to its
    /// ancestors, so a custom search UI can take it.
    pub(super) fn on_action_search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        if !self.searchable {
            cx.propagate();
            return;
        }
        self.open_search(false, cx);
    }

    pub(super) fn on_action_replace(
        &mut self,
        _: &Replace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.searchable {
            cx.propagate();
            return;
        }
        self.open_search(true, cx);
    }
}

impl Default for SearchMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchMatcher {
    pub fn new() -> Self {
        Self {
            text: "".into(),
            query: None,
            matches: Matches::default(),
            current_match_ix: 0,
            replacing: false,
            tracking: false,
            pending: None,
        }
    }

    /// Update the source text and recompute matches.
    pub fn update(&mut self, text: &Rope) {
        self.tracking = false;
        if self.text.eq(text) {
            self.replacing = false;
            return;
        }
        self.text = text.clone();
        self.update_matches();
    }

    /// [`Self::update`] for the input's own text, which skips comparing it
    /// while every edit to it has reached the matcher.
    pub(crate) fn sync(&mut self, text: &Rope) {
        if self.tracking {
            self.replacing = false;
            return;
        }
        self.update(text);
        self.tracking = true;
    }

    /// Take the input's `text` without searching it, for a new query to.
    pub(crate) fn take_text(&mut self, text: &Rope) {
        self.text = text.clone();
        self.tracking = true;
    }

    /// The text changed in a way no edit describes.
    pub(crate) fn untrack(&mut self) {
        self.tracking = false;
        // Nor could a background search catch up with it.
        self.pending = None;
    }

    /// Bytes `range` of the text were replaced by `new_len` bytes, making
    /// `text`. Only the matches the edit can have changed are searched for
    /// again, unless the matcher has lost track of the text.
    pub(crate) fn edited(&mut self, text: &Rope, range: Range<usize>, new_len: usize) {
        if !self.tracking {
            return;
        }
        self.text = text.clone();
        if let Some(pending) = &mut self.pending {
            let edit = (range.clone(), new_len);
            pending.edit = Some(match pending.edit.take() {
                Some(before) => compose_edits(before, edit),
                None => edit,
            });
        }
        let Some(query) = &self.query else {
            return;
        };
        let matches = edit_matches(query, text, &self.matches, range, new_len);
        self.set_matches(matches);
    }

    pub fn update_query(&mut self, query: &str, case_insensitive: bool) {
        self.set_query(query, case_insensitive);
        self.update_matches();
    }

    fn set_query(&mut self, query: &str, case_insensitive: bool) {
        self.query = (!query.is_empty()).then(|| {
            AhoCorasick::builder()
                .ascii_case_insensitive(case_insensitive)
                .build([query])
                .expect("failed to build input search query")
        });
    }

    /// [`Self::update_query`], searching at most [`SYNC_SEARCH_BYTES`] at
    /// once: in a longer text those from `anchor` on, the matches found there
    /// standing in for all of them until [`Self::finish_full_search`] takes
    /// the returned search's.
    pub(crate) fn update_query_from(
        &mut self,
        query: &str,
        case_insensitive: bool,
        anchor: usize,
    ) -> Option<FullSearch> {
        if query.is_empty() || self.text.len() <= SYNC_SEARCH_BYTES {
            self.update_query(query, case_insensitive);
            return None;
        }
        self.set_query(query, case_insensitive);
        let query = self.query.clone()?;
        let from = anchor.min(self.text.len());
        let found = find_all(
            &query,
            &self.text,
            from,
            from + SYNC_SEARCH_BYTES,
            &mut Vec::new(),
        );
        self.set_matches(Matches::from_ranges(found));
        self.pending = Some(PendingSearch {
            anchor: from,
            edit: None,
        });
        Some(FullSearch {
            query,
            text: self.text.clone(),
        })
    }

    /// Take the matches a background search found, searched again around
    /// the edits made since it started. The current match stays where it
    /// was. Returns whether it took them: not once the matcher searched the
    /// whole text itself, or lost track of it.
    pub(crate) fn finish_full_search(&mut self, found: FullSearchMatches) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        let Some(query) = &self.query else {
            return false;
        };
        let mut matches = found.0;
        if let Some((range, new_len)) = pending.edit {
            matches = edit_matches(query, &self.text, &matches, range, new_len);
        }
        let current = self
            .current()
            .and_then(|ix| self.matches.get(ix))
            .map(|range| range.start);
        self.matches = matches;
        match current {
            Some(start) => {
                self.current_match_ix = self
                    .matches
                    .starting_before(start)
                    .min(self.len().saturating_sub(1));
            }
            None => {
                self.current_match_ix = 0;
                self.update_cursor_by_offset(pending.anchor);
            }
        }
        true
    }

    /// Search the whole text now if a background search is under way.
    pub(crate) fn complete(&mut self) {
        if self.pending.is_some() {
            self.update_matches();
        }
    }

    /// Every match, copied out: see [`Self::match_at`] for one.
    pub fn matched_ranges(&self) -> Vec<Range<usize>> {
        self.matches.iter().collect()
    }

    /// The match at `ix` in text order.
    pub fn match_at(&self, ix: usize) -> Option<Range<usize>> {
        self.matches.get(ix)
    }

    /// The matches that reach into `range`, with their indices.
    pub(crate) fn matches_in(
        &self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (usize, Range<usize>)> + '_ {
        self.matches.within(range)
    }

    pub fn current_match_index(&self) -> usize {
        self.current_match_ix
    }

    /// The index of the current match into [`SearchMatcher::matched_ranges`],
    /// `None` while there is no match.
    pub fn current(&self) -> Option<usize> {
        (!self.is_empty()).then_some(self.current_match_ix)
    }

    pub fn len(&self) -> usize {
        self.matches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `2/5`: the current match and the total, `0/0` without matches.
    pub fn label(&self) -> String {
        match self.current() {
            Some(ix) => format!("{}/{}", ix + 1, self.len()),
            None => "0/0".into(),
        }
    }

    fn peek(&self) -> Option<Range<usize>> {
        self.next_index().and_then(|ix| self.matches.get(ix))
    }

    fn has_next_without_wrap(&self) -> bool {
        self.current_match_ix < self.len().saturating_sub(1)
    }

    /// Make the first match that ends at `offset` or after it current, or
    /// the last match when none does.
    pub fn update_cursor_by_offset(&mut self, offset: usize) {
        if self.is_empty() {
            return;
        }
        let ending_before = offset
            .checked_sub(1)
            .map_or(0, |before| self.matches.ending_by(before));
        self.current_match_ix = ending_before.min(self.len() - 1);
    }

    /// Preserve the current logical match while a replacement mutates text.
    fn begin_replacement(&mut self) {
        self.replacing = true;
    }

    fn set_current_match_index(&mut self, index: usize) {
        self.current_match_ix = index.min(self.len().saturating_sub(1));
    }

    fn next_index(&self) -> Option<usize> {
        if self.is_empty() {
            None
        } else if self.has_next_without_wrap() {
            Some(self.current_match_ix + 1)
        } else {
            Some(0)
        }
    }

    fn update_matches(&mut self) {
        // A search of everything supersedes one under way.
        self.pending = None;
        let matches = match &self.query {
            Some(query) => Matches::from_ranges(find_all(
                query,
                &self.text,
                0,
                self.text.len(),
                &mut Vec::new(),
            )),
            None => Matches::default(),
        };
        self.set_matches(matches);
    }

    fn set_matches(&mut self, matches: Matches) {
        self.matches = matches;
        if !self.replacing || self.is_empty() {
            self.current_match_ix = 0;
        } else {
            self.current_match_ix = self.current_match_ix.min(self.len() - 1);
        }
        self.replacing = false;
    }
}

/// Hands `found` the matches of `query` in `text` that start in `from..until`,
/// left to right and without overlaps, until it breaks. `from` must not be
/// inside a match. The text is searched a window at a time, copied into
/// `window`, so the document is never copied whole.
fn search_rope(
    query: &AhoCorasick,
    text: &Rope,
    from: usize,
    until: usize,
    window: &mut Vec<u8>,
    mut found: impl FnMut(Range<usize>) -> ControlFlow<()>,
) {
    let reach = query.max_pattern_len().saturating_sub(1);
    let until = until.min(text.len());
    // No match starts inside a character.
    let mut pos = text.ceil_char_boundary(from.min(until));
    while pos < until {
        let starts_end = text.ceil_char_boundary((pos + SEARCH_WINDOW).min(until));
        let end = text.ceil_char_boundary((starts_end + reach).min(text.len()));
        window.clear();
        for chunk in text.slice(pos..end).chunks() {
            window.extend_from_slice(chunk.as_bytes());
        }
        let mut next = starts_end;
        for found_match in query.find_iter(&window[..]) {
            let start = pos + found_match.start();
            if start >= starts_end {
                break;
            }
            let end = pos + found_match.end();
            if found(start..end).is_break() {
                return;
            }
            next = next.max(end);
        }
        pos = next;
    }
}

/// The matches of `query` in `text` starting in `from..until`, see
/// [`search_rope`].
fn find_all(
    query: &AhoCorasick,
    text: &Rope,
    from: usize,
    until: usize,
    window: &mut Vec<u8>,
) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    search_rope(query, text, from, until, window, |found| {
        ranges.push(found);
        ControlFlow::Continue(())
    });
    ranges
}

/// The first match of `query` in `text` starting in `from..until`, see
/// [`search_rope`].
fn first_match(
    query: &AhoCorasick,
    text: &Rope,
    from: usize,
    until: usize,
    window: &mut Vec<u8>,
) -> Option<Range<usize>> {
    let mut first = None;
    search_rope(query, text, from, until, window, |found| {
        first = Some(found);
        ControlFlow::Break(())
    });
    first
}

/// The one edit that does what `first` and then `second` do, each a range of
/// the text before it replaced by so many bytes: the bytes either touched.
fn compose_edits(
    (first, first_len): (Range<usize>, usize),
    (second, second_len): (Range<usize>, usize),
) -> (Range<usize>, usize) {
    let start = first.start.min(second.start);
    // Where the bytes either edit touched end, in the text between the two.
    let first_end = first.start + first_len;
    let end = first_end.max(second.end);
    let old_end = end - first_end + first.end;
    let new_end = end - second.end + second.start + second_len;
    (start..old_end, new_end - start)
}

/// The matches of `query` in `text`, which was the text `matches` were
/// found in with bytes `range` replaced by `new_len` bytes.
///
/// Matches out of the edit's reach move with it, which takes changing the
/// first of them alone. Around it, the text is searched again from where no
/// match could have changed, and on until a match lines up with one found
/// before it, or the search finds the text as unmatched as it was: from there
/// on, matching left to right without overlaps goes as it went.
fn edit_matches(
    query: &AhoCorasick,
    text: &Rope,
    matches: &Matches,
    range: Range<usize>,
    new_len: usize,
) -> Matches {
    let reach = query.max_pattern_len().saturating_sub(1);
    let edit_end = range.start + new_len;
    let mut cursor = matches.0.cursor::<MatchEnd>(());
    // Matches wholly before the edit keep their bytes.
    let mut edited = cursor.slice(&MatchEnd(range.start), Bias::Right);
    let mut end = edited.summary().end;
    // Those that start inside it are gone.
    while let Some(found) = cursor.item()
        && cursor.start().0 + found.gap < range.end
    {
        cursor.next();
    }
    // Where the match at the cursor is now, moved with the edit.
    let moved = |cursor_end: usize, found: &Match| {
        let start = cursor_end + found.gap - range.end + edit_end;
        start..start + found.len
    };

    // A match can start this far before the edit and reach into it.
    let mut pos = range.start.saturating_sub(reach).max(end);
    let mut until = (edit_end + reach).min(text.len());
    let mut window = Vec::new();
    loop {
        // Matches from before that overlap the ones found again are gone,
        // and the search goes on to where they ended.
        while let Some(found) = cursor.item() {
            let kept = moved(cursor.start().0, found);
            if kept.start >= pos {
                break;
            }
            until = until.max(kept.end);
            cursor.next();
        }
        if pos >= until {
            break;
        }
        let Some(found) = first_match(query, text, pos, until, &mut window) else {
            break;
        };
        pos = found.end;
        edited.push(
            Match {
                gap: found.start - end,
                len: found.len(),
            },
            (),
        );
        end = found.end;
    }
    // The first match kept starts so far after the last one now.
    if let Some(found) = cursor.item() {
        let kept = moved(cursor.start().0, found);
        edited.push(
            Match {
                gap: kept.start - end,
                len: found.len,
            },
            (),
        );
        cursor.next();
    }
    edited.append(cursor.suffix(), ());
    Matches(edited)
}

impl Iterator for SearchMatcher {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let ix = self.next_index()?;
        self.current_match_ix = ix;
        self.matches.get(ix)
    }
}

impl DoubleEndedIterator for SearchMatcher {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }
        if self.current_match_ix == 0 {
            self.current_match_ix = self.len();
        }
        self.current_match_ix -= 1;
        self.matches.get(self.current_match_ix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_matches(query: &str, text: &str) -> Vec<Range<usize>> {
        let query = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build([query])
            .unwrap();
        query.find_iter(text).map(|found| found.range()).collect()
    }

    /// Matches found a window at a time are the ones found in one go, the
    /// ones across a window's end included.
    #[test]
    fn a_search_across_windows_finds_what_one_pass_finds() {
        let mut text = "x".repeat(SEARCH_WINDOW - 3);
        text.push_str("needle needle");
        text.push_str(&"é".repeat(SEARCH_WINDOW));
        text.push_str("NEEDLE");
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from(text.as_str()));
        matcher.update_query("needle", true);
        assert_eq!(matcher.matched_ranges(), all_matches("needle", &text));
        assert_eq!(matcher.len(), 3);
    }

    /// After each edit the matches are what searching the whole text again
    /// finds, for a query that can overlap itself too, whose matches an edit
    /// can move far past it.
    #[test]
    fn edits_search_again_only_around_them_and_agree_with_a_full_search() {
        for (query, start, edits) in [
            (
                "cx.notify()",
                "a cx.notify() b cx.notify()\ncx.notify() cx.notify()",
                vec![
                    (3..4, "X"),
                    (0..0, "cx.notify() "),
                    (14..20, ""),
                    (30..30, "cx.no"),
                    (35..35, "tify()"),
                ],
            ),
            (
                "aa",
                "aaaaaaaaa b aaaa",
                vec![(0..0, "a"), (1..2, ""), (5..5, "b"), (5..6, ""), (16..16, "aaa")],
            ),
        ] {
            let mut text = Rope::from(start);
            let mut matcher = SearchMatcher::new();
            matcher.take_text(&text);
            matcher.update_query(query, true);
            for (range, new_text) in edits {
                text.replace(range.clone(), new_text);
                matcher.edited(&text, range.clone(), new_text.len());
                assert_eq!(
                    matcher.matched_ranges(),
                    all_matches(query, &text.to_string()),
                    "{query:?} after replacing {range:?} with {new_text:?} in {text}"
                );
            }
        }
    }

    /// Many edits at random places, in a text of few letters, for queries
    /// that overlap themselves: the matches always agree with a full search.
    #[test]
    fn random_edits_agree_with_a_full_search() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        for query in ["aa", "aba", "abab", "b"] {
            let mut text = Rope::from("abaababaaab".repeat(8).as_str());
            let mut matcher = SearchMatcher::new();
            matcher.take_text(&text);
            matcher.update_query(query, true);
            for _ in 0..300 {
                let start = next(text.len() + 1);
                let end = (start + next(5)).min(text.len());
                let new_text = ["", "a", "b", "ab", "aab", "ba"][next(6)];
                text.replace(start..end, new_text);
                matcher.edited(&text, start..end, new_text.len());
                assert_eq!(
                    matcher.matched_ranges(),
                    all_matches(query, &text.to_string()),
                    "{query:?} after replacing {:?} with {new_text:?}",
                    start..end
                );
            }
        }
    }

    /// A new query in a text longer than the main thread searches shows the
    /// matches from its anchor on at once, and every match once the search
    /// of the whole text lands, the edits made meanwhile applied to them,
    /// the current match staying put.
    #[test]
    fn a_long_text_is_searched_whole_in_the_background() {
        let filler = "x".repeat(SYNC_SEARCH_BYTES / 2);
        let start = format!("needle {filler} needle {filler}needle{filler}{filler}needle needle");
        let mut text = Rope::from(start.as_str());
        let anchor = SYNC_SEARCH_BYTES / 2;
        let mut matcher = SearchMatcher::new();
        matcher.take_text(&text);
        let full = matcher
            .update_query_from("needle", true, anchor)
            .expect("a text past the budget is searched in the background");
        let near: Vec<_> = all_matches("needle", &start)
            .into_iter()
            .filter(|found| (anchor..anchor + SYNC_SEARCH_BYTES).contains(&found.start))
            .collect();
        assert_eq!(matcher.matched_ranges(), near);
        assert_eq!(near.len(), 2);

        for (range, new_text) in [(0..0, "needle "), (14..20, ""), (8..8, "nee")] {
            text.replace(range.clone(), new_text);
            matcher.edited(&text, range, new_text.len());
        }
        matcher.set_current_match_index(matcher.len() - 1);
        let current = matcher.matched_ranges()[matcher.current_match_index()].clone();
        assert!(matcher.finish_full_search(full.run(&AtomicBool::new(false)).unwrap()));
        assert_eq!(
            matcher.matched_ranges(),
            all_matches("needle", &text.to_string())
        );
        assert_eq!(
            matcher.matched_ranges()[matcher.current_match_index()],
            current
        );
    }

    /// A background search stops once its task is dropped, as a newer
    /// search drops it, and the matcher drops the matches of one it
    /// completed itself.
    #[test]
    fn a_superseded_background_search_stops() {
        let text = Rope::from(format!("{}a b", "x".repeat(SYNC_SEARCH_BYTES)).as_str());
        let mut matcher = SearchMatcher::new();
        matcher.take_text(&text);
        let first = matcher.update_query_from("a", true, 0).unwrap();
        let cancel = CancelOnDrop::new();
        let flag = cancel.flag();
        drop(cancel);
        assert!(first.run(&flag).is_none());

        let second = matcher.update_query_from("b", true, 0).unwrap();
        assert!(matcher.finish_full_search(second.run(&AtomicBool::new(false)).unwrap()));
        assert_eq!(matcher.len(), 1);

        let third = matcher.update_query_from("a", true, 0).unwrap();
        assert!(matcher.is_empty());
        matcher.complete();
        assert_eq!(matcher.len(), 1);
        assert!(!matcher.finish_full_search(third.run(&AtomicBool::new(false)).unwrap()));
    }

    /// Edits at random places while a background search is under way are
    /// searched again around all at once when it lands, against the text
    /// they made, and the matches agree with a search of that text whole.
    #[test]
    fn edits_during_a_background_search_agree_with_a_full_search() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        let start = "abaababaaab".repeat(SYNC_SEARCH_BYTES / 11 + 64);
        for query in ["aa", "abab"] {
            for edits in [1, 2, 5, 12] {
                let mut text = Rope::from(start.as_str());
                let mut matcher = SearchMatcher::new();
                matcher.take_text(&text);
                let anchor = next(text.len());
                let full = matcher
                    .update_query_from(query, true, anchor)
                    .expect("a text past the budget is searched in the background");
                for _ in 0..edits {
                    let start = next(text.len() + 1);
                    let end = (start + next(5)).min(text.len());
                    let new_text = ["", "a", "b", "ab", "aab", "ba"][next(6)];
                    text.replace(start..end, new_text);
                    matcher.edited(&text, start..end, new_text.len());
                }
                assert!(matcher.finish_full_search(full.run(&AtomicBool::new(false)).unwrap()));
                assert!(
                    matcher.matched_ranges() == all_matches(query, &text.to_string()),
                    "{query:?} after {edits} edits"
                );
            }
        }
    }

    /// A matcher that lost track of the text searches it all again.
    #[test]
    fn an_untracked_change_searches_again() {
        let mut matcher = SearchMatcher::new();
        let text = Rope::from("foo foo");
        matcher.take_text(&text);
        matcher.update_query("foo", true);
        matcher.untrack();
        let text = Rope::from("foo bar foo foo");
        matcher.edited(&text, 0..0, 8);
        matcher.sync(&text);
        assert_eq!(matcher.len(), 3);
    }

    #[test]
    fn finds_navigates_and_preserves_replacement_position() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo FOO foo"));
        matcher.update_query("foo", true);
        assert_eq!(&matcher.matched_ranges(), &[0..3, 4..7, 8..11]);
        assert_eq!(matcher.next(), Some(4..7));
        assert_eq!(matcher.next_back(), Some(0..3));

        matcher.set_current_match_index(2);
        matcher.begin_replacement();
        matcher.update(&Rope::from("foo FOO bar"));
        assert_eq!(matcher.current_match_index(), 1);
    }

    #[test]
    fn next_wraps_to_start() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from(".....aaaaa.....aaaaa.....aaaaa"));
        matcher.update_query("aaaaa", false);
        matcher.set_current_match_index(2);
        assert_eq!(matcher.next(), Some(5..10));
    }

    #[test]
    fn a_query_set_without_the_panel_keeps_the_session_active_until_closed() {
        let mut session = SearchSession::default();
        assert!(!session.is_active());

        session.open(false, true);
        assert!(session.is_active());
        session.close();
        assert!(!session.is_active());

        // A custom search UI never opens the panel; setting a query is what
        // turns the match highlights on, and closing turns them off again.
        session.activate();
        assert!(session.is_active());
        assert!(!session.open);
        session.close();
        assert!(!session.is_active());
    }

    #[test]
    fn identical_query_keeps_the_current_match() {
        let mut session = SearchSession::default();
        session.update_query("foo", true);
        session.matcher.update(&Rope::from("foo bar foo baz foo"));
        session.matcher.update_cursor_by_offset(12);
        assert_eq!(session.matcher.current_match_index(), 2);

        // Reopening Find and the styled search panel's initial query echo both
        // update the session with the same query. Neither should reset the
        // previously active occurrence.
        session.update_query("foo", true);

        assert_eq!(session.matcher.current_match_index(), 2);
        assert_eq!(session.matcher.label(), "3/3");
    }

    #[test]
    fn replacement_keeps_current_match_index_on_next_match() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo foo foo"));
        matcher.update_query("foo", true);
        assert_eq!(matcher.label(), "1/3");

        assert!(matcher.has_next_without_wrap());
        matcher.begin_replacement();
        matcher.update(&Rope::from("bar foo foo"));
        assert_eq!(matcher.current_match_index(), 0);
        assert_eq!(matcher.matched_ranges()[0], 4..7);
        assert_eq!(matcher.label(), "1/2");

        matcher.set_current_match_index(1);
        assert!(!matcher.has_next_without_wrap());
        matcher.set_current_match_index(0);
        matcher.begin_replacement();
        matcher.update(&Rope::from("bar foo bar"));
        assert_eq!(matcher.current_match_index(), 0);
        assert_eq!(matcher.matched_ranges()[0], 4..7);
        assert_eq!(matcher.label(), "1/1");
    }

    #[test]
    fn update_matches_clamps_current_match_index_while_replacing() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo foo foo"));
        matcher.update_query("foo", true);
        matcher.set_current_match_index(2);
        matcher.begin_replacement();

        matcher.update(&Rope::from("foo xoo foo"));

        assert_eq!(matcher.len(), 2);
        assert_eq!(matcher.current_match_index(), 1);
        assert_eq!(matcher.label(), "2/2");
    }
}

//! What a person did that the new-tab launcher offers again (`docs/new-tab.md`): the pages browser
//! tabs showed, the searches made, what was said to chats and the commands run in terminals. Each
//! is kept with its latest uses, how each came about and where, and what was typed before each was
//! picked from the launcher, ranked as Firefox's address bar ranks its places (`rank`). One history
//! for every project, those used in the checkout at hand ranked higher. It's kept in a file in the
//! app's data folder, written a moment after a change settles.

mod icons;
mod rank;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::{App, Global, Task};
use serde::{Deserialize, Serialize};

pub use icons::{icon, keep_icon};
pub use rank::{Found, Origins, display_url, match_words, same_page_key};

use crate::{atomic_file, settings};

/// What the file is called, in the app's data folder.
const FILE: &str = "history.json";
/// How long after a change the history is written, with whatever else changed meanwhile.
const SAVE_DELAY: Duration = Duration::from_secs(1);
/// The most uses an entry keeps; its frecency samples them (`rank`).
pub(crate) const SAMPLED: usize = 10;
/// The most checkouts and projects an entry remembers being used in.
const PLACES: usize = 4;
/// The longest a message is kept, in characters: one longer is a document, not a thing to say
/// again.
const LONGEST: usize = 2000;
/// How many entries of each kind are kept: past that, the least frecent go.
const KEPT_PAGES: usize = 5000;
const KEPT_WORDS: usize = 1000;
/// How many inputs picked from are learned; past that, the least used are forgotten.
const KEPT_LEARNED: usize = 1000;

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// A page a browser tab showed, by its address.
    Page,
    /// Words searched for from the launcher or a browser tab's address.
    Search,
    /// A message said to a chat.
    Prompt,
    /// A command run in a terminal from the launcher.
    Command,
}

/// How a use came about, which weighs on how much it counts (`rank::weight`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum How {
    /// Typed, or picked from the launcher: the person asked for it by name.
    Typed,
    /// A link followed, or a page that went on to another.
    Followed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Use {
    /// When, in seconds since 1970.
    pub at: i64,
    pub how: How,
}

/// Something done, as it's suggested again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: Kind,
    /// The page's address, or the words.
    pub text: String,
    /// A page's title; empty for words, and for a page that has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// The latest uses, oldest first, at most [`SAMPLED`].
    pub uses: Vec<Use>,
    /// How many times it was used in all.
    pub count: u32,
    /// The checkouts it was used in lately, and their projects' main checkouts, the latest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worktrees: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<PathBuf>,
}

impl Entry {
    /// When it was last used.
    pub fn last_used(&self) -> i64 {
        self.uses.last().map_or(0, |used| used.at)
    }
}

/// What was typed before an entry was picked from the launcher: typing it again offers that entry
/// first, as Firefox's adaptive history does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    pub kind: Kind,
    /// What was typed, lowercased.
    pub input: String,
    /// The entry picked, by its text.
    pub text: String,
    /// How much it was picked: a pick adds one to nine tenths of what it was.
    pub uses: f32,
    /// When it was last picked, in seconds since 1970: it fades as days pass (`rank::faded`).
    pub at: i64,
}

/// Where something was done: the checkout, and its project's main checkout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Place {
    pub worktree: PathBuf,
    pub project: PathBuf,
}

/// The history, written as its entries and what was learned, and where it's kept.
#[derive(Default, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default)]
    learned: Vec<Learned>,
    /// The project each checkout listed belongs to, by its main checkout.
    #[serde(skip)]
    projects: HashMap<PathBuf, PathBuf>,
    #[serde(skip)]
    path: Option<PathBuf>,
    /// The write coming, which a change made meanwhile joins.
    #[serde(skip)]
    saving: Option<Task<()>>,
}

impl Global for History {}

/// The time now, in seconds since 1970.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// Reads the history from the app's data folder; tests keep theirs in memory.
pub fn init(cx: &mut App) {
    let path = settings::data_dir()
        .filter(|_| !cfg!(test))
        .map(|data| data.join(FILE));
    let history = History { path: path.clone(), ..path.as_deref().map(load).unwrap_or_default() };
    cx.set_global(history);
    // A change still settling when the app quits is written on the way out.
    cx.on_app_quit(|cx| {
        let history = cx.global_mut::<History>();
        if history.saving.take().is_some()
            && let Some(path) = history.path.clone()
        {
            atomic_file::write(&path, &history.encode());
        }
        async {}
    })
    .detach();
}

fn load(path: &Path) -> History {
    let Ok(json) = std::fs::read(path) else {
        return History::default();
    };
    serde_json::from_slice(&json).unwrap_or_else(|error| {
        log::warn!("couldn't read the history at {}: {error}", path.display());
        History::default()
    })
}

/// The history, to read.
pub fn get(cx: &App) -> &History {
    cx.global::<History>()
}

/// Changes the history with `change`, and writes it a moment later, with the changes made
/// meanwhile: changes as steady as a page's can't put the write off for good.
fn update(cx: &mut App, change: impl FnOnce(&mut History)) {
    if cx.try_global::<History>().is_none() {
        return;
    }
    change(cx.global_mut::<History>());
    let history = cx.global::<History>();
    if history.path.is_none() || history.saving.is_some() {
        return;
    }
    let save = cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DELAY).await;
        let encoded = cx.update(|cx| {
            let history = cx.global_mut::<History>();
            // Dropped, the handle would cancel this very write; a change from now on is the
            // next one's.
            if let Some(save) = history.saving.take() {
                save.detach();
            }
            Some((history.path.clone()?, history.encode()))
        });
        if let Some((path, json)) = encoded {
            cx.background_executor().spawn(async move { atomic_file::write(&path, &json) }).await;
        }
    });
    cx.global_mut::<History>().saving = Some(save);
}

/// A browser tab loaded the page at `url`, titled `title`, in `place`, having come `how`: a
/// visit. Only web pages and files are kept, not the engine's own.
pub fn visited(url: &str, title: &str, how: How, place: &Place, cx: &mut App) {
    if !kept_page(url) {
        return;
    }
    update(cx, |history| {
        let entry = history.used(Kind::Page, url, how, place);
        if !title.is_empty() {
            title.clone_into(&mut entry.title);
        }
    });
}

/// The page at `url` is titled `title` now, as a tab shows it again or its title changes: no
/// visit, as Firefox counts none for a reload or its session history.
pub fn retitled(url: &str, title: &str, cx: &mut App) {
    let stale = cx.try_global::<History>().is_some_and(|history| {
        !title.is_empty() && history.of(Kind::Page).any(|entry| entry.text == url && entry.title != title)
    });
    if stale {
        update(cx, |history| {
            if let Some(entry) = history.find_mut(Kind::Page, url) {
                title.clone_into(&mut entry.title);
            }
        });
    }
}

/// Tells the history the checkouts listed, each with its project's main checkout, so what's done
/// in a checkout is known to its project too.
pub fn know_checkouts(checkouts: impl IntoIterator<Item = (PathBuf, PathBuf)>, cx: &mut App) {
    if let Some(history) = cx.try_global::<History>() {
        let projects: HashMap<PathBuf, PathBuf> = checkouts.into_iter().collect();
        if history.projects != projects {
            cx.global_mut::<History>().projects = projects;
        }
    }
}

/// Where something done in the checkout at `worktree` was done.
pub fn place_of(worktree: &Path, cx: &App) -> Place {
    let project = cx.try_global::<History>().and_then(|history| history.projects.get(worktree));
    Place {
        worktree: worktree.to_owned(),
        project: project.cloned().unwrap_or_else(|| worktree.to_owned()),
    }
}

/// Whether a page at `url` is one to remember: pages on the web and files, not the engine's own.
fn kept_page(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (crate::browser::LINK_SCHEMES.iter())
        .filter(|scheme| **scheme != "about:")
        .any(|scheme| lower.starts_with(scheme))
}

/// Words `text` were said, searched or run, as `kind`, in `place`.
pub fn used(kind: Kind, text: &str, place: &Place, cx: &mut App) {
    let text = text.trim();
    if text.is_empty() || kind == Kind::Page {
        return;
    }
    let text: String = text.chars().take(LONGEST).collect();
    update(cx, |history| {
        history.used(kind, &text, How::Typed, place);
    });
}

/// The person typed `input` in the launcher and picked the entry of `kind` with `text`.
pub fn picked(kind: Kind, input: &str, text: &str, cx: &mut App) {
    let input = input.trim().to_lowercase();
    if input.is_empty() {
        return;
    }
    update(cx, |history| history.learn(kind, input, text, now()));
}

/// Forgets the entry of `kind` with `text`, and what was learned of it.
pub fn remove(kind: Kind, text: &str, cx: &mut App) {
    update(cx, |history| {
        history.entries.retain(|entry| !(entry.kind == kind && entry.text == text));
        history.learned.retain(|learned| !(learned.kind == kind && learned.text == text));
    });
}

/// Forgets the pages and searches, as clearing the browser's data does.
pub fn clear_browsing(cx: &mut App) {
    update(cx, |history| {
        let browsing = |kind: Kind| matches!(kind, Kind::Page | Kind::Search);
        history.entries.retain(|entry| !browsing(entry.kind));
        history.learned.retain(|learned| !browsing(learned.kind));
    });
    icons::clear(cx);
}

impl History {
    /// What's written: the entries and what was learned, borrowed as they are.
    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Every entry of `kind`.
    pub fn of(&self, kind: Kind) -> impl DoubleEndedIterator<Item = &Entry> {
        self.entries.iter().filter(move |entry| entry.kind == kind)
    }

    fn find_mut(&mut self, kind: Kind, text: &str) -> Option<&mut Entry> {
        (self.entries.iter_mut()).find(|entry| entry.kind == kind && entry.text == text)
    }

    /// Notes a use of the entry of `kind` with `text`, made now if it's new, and gives it back.
    fn used(&mut self, kind: Kind, text: &str, how: How, place: &Place) -> &mut Entry {
        self.used_at(kind, text, Use { at: now(), how }, place)
    }

    fn used_at(&mut self, kind: Kind, text: &str, used: Use, place: &Place) -> &mut Entry {
        // The entry used last goes last, so entries used within the same second keep their order.
        let found = self.entries.iter().position(|entry| entry.kind == kind && entry.text == text);
        let entry = if let Some(at) = found {
            self.entries.remove(at)
        } else {
            self.make_room(kind);
            Entry {
                kind,
                text: text.to_owned(),
                title: String::new(),
                uses: Vec::new(),
                count: 0,
                worktrees: Vec::new(),
                projects: Vec::new(),
            }
        };
        self.entries.push(entry);
        let last = self.entries.len() - 1;
        let entry = &mut self.entries[last];
        entry.count = entry.count.saturating_add(1);
        entry.uses.push(used);
        if entry.uses.len() > SAMPLED {
            entry.uses.remove(0);
        }
        latest_first(&mut entry.worktrees, &place.worktree);
        latest_first(&mut entry.projects, &place.project);
        entry
    }

    /// Lets the least frecent entry of `kind` go when there are as many as are kept.
    fn make_room(&mut self, kind: Kind) {
        let kept = if kind == Kind::Page { KEPT_PAGES } else { KEPT_WORDS };
        if self.of(kind).count() < kept {
            return;
        }
        let now = now();
        let least = (self.entries.iter().enumerate())
            .filter(|(_, entry)| entry.kind == kind)
            .map(|(at, entry)| (at, rank::strength(entry, now)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(at, _)| at);
        if let Some(at) = least {
            // Removed in place, so the entries keep the order they were last used in.
            let gone = self.entries.remove(at);
            self.learned.retain(|learned| !(learned.kind == kind && learned.text == gone.text));
        }
    }

    /// Learns that `input` led to the entry of `kind` with `text`, at `at`.
    fn learn(&mut self, kind: Kind, input: String, text: &str, at: i64) {
        let known = (self.learned.iter_mut())
            .find(|learned| learned.kind == kind && learned.input == input && learned.text == text);
        if let Some(learned) = known {
            learned.uses = rank::faded(learned, at) * 0.9 + 1.;
            learned.at = at;
            return;
        }
        if self.learned.len() >= KEPT_LEARNED
            && let Some(least) = (self.learned.iter().enumerate())
                .min_by(|a, b| rank::faded(a.1, at).total_cmp(&rank::faded(b.1, at)))
                .map(|(at, _)| at)
        {
            self.learned.swap_remove(least);
        }
        self.learned.push(Learned { kind, input, text: text.to_owned(), uses: 1., at });
    }
}

/// Puts `path` first in `list`, keeping at most [`PLACES`].
fn latest_first(list: &mut Vec<PathBuf>, path: &Path) {
    if path.as_os_str().is_empty() {
        return;
    }
    list.retain(|kept| kept != path);
    list.insert(0, path.to_owned());
    list.truncate(PLACES);
}

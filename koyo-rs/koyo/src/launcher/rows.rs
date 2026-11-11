//! What the launcher lists for what's typed (`docs/new-tab.md` 3), grouped as Firefox's address
//! bar groups its results. Typed, the first row does it: visits the address, searches the web,
//! asks a new chat or runs the command. A browser then lists searches, those made before and the
//! search engine's suggestions, and after them the pages: those picked before after typing the
//! same, then the history's best, a page a tab shows already being switched to. A chat lists its
//! tabs, its chats no tab shows and what was said before; a terminal its tabs and the commands
//! run before. With nothing typed, a browser lists its tabs and the top sites, a chat its chats
//! and what was said lately, a terminal the commands run lately. A device lists its tabs and
//! Koyo's simulators, typed or not.

use std::ops::Range;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::{App, Image, SharedString};
use koyo_agent::protocol::ids::ThreadId;

use super::Kind;
use super::here::{Here, OpenTab};
use super::suggest::is_address;
use crate::browser::{Address, address_to_url, read_address};
use crate::devices::Chosen;
use crate::history::{self, Found};
use crate::model::PaneKind;

/// The most rows the launcher lists.
pub const MOST: usize = 10;
/// With nothing typed, the most rows, as Firefox lists eight top sites; and the most tabs or chats
/// among them.
const MOST_EMPTY: usize = 8;
const MOST_EMPTY_TABS: usize = 4;
/// The most searches made before a browser lists, as Firefox does.
const MOST_PAST_SEARCHES: usize = 2;
/// How many matches the history is asked for, past what's shown: some may be tabs open already.
const ASKED: usize = 15;

/// What picking a row does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Opens an empty tab of the kind.
    Empty(Kind),
    Visit(String),
    Search(String),
    /// Shows the tab holding this pane.
    Switch(u64),
    /// Shows this chat, which no tab shows.
    Resume(ThreadId),
    /// Says this to a new chat.
    Ask(String),
    /// Runs this in a new terminal.
    Run(String),
    /// Opens the New Worktree dialog with this name.
    StartWorktree(String),
    /// Shows this device in a tab of its own, started.
    Device(Chosen),
}

/// What marks a row: an icon, or a page's own.
#[derive(Clone)]
pub enum Glyph {
    Icon(IconName),
    Image(Arc<Image>),
}

#[derive(Clone)]
pub struct Row {
    pub glyph: Glyph,
    pub title: SharedString,
    pub title_matched: Vec<Range<usize>>,
    /// Said dimmed after the title: a page's address.
    pub detail: SharedString,
    pub detail_matched: Vec<Range<usize>>,
    /// What picking it does, at its end: "Switch to Tab".
    pub hint: Option<&'static str>,
    pub act: Act,
    /// The history entry it shows, which ⇧⌫ or its × forgets.
    pub entry: Option<(history::Kind, String)>,
}

impl Row {
    fn new(glyph: Glyph, title: impl Into<SharedString>, act: Act) -> Row {
        Row {
            glyph,
            title: title.into(),
            title_matched: Vec::new(),
            detail: SharedString::default(),
            detail_matched: Vec::new(),
            hint: None,
            act,
            entry: None,
        }
    }

    fn hint(mut self, hint: &'static str) -> Row {
        self.hint = Some(hint);
        self
    }

    /// The text it puts in the field when → takes it.
    pub fn text(&self) -> Option<&str> {
        match &self.act {
            Act::Visit(text) | Act::Search(text) | Act::Ask(text) | Act::Run(text) => Some(text),
            Act::Empty(_)
            | Act::Switch(_)
            | Act::Resume(_)
            | Act::StartWorktree(_)
            | Act::Device(_) => None,
        }
    }
}

/// What the rows are made for.
pub struct Query<'a> {
    pub kind: Kind,
    /// What the person typed, a completion aside.
    pub typed: &'a str,
    /// The host completing what's typed, in the field after it.
    pub autofilled: Option<&'a str>,
    pub here: &'a Here,
    /// The search engine's suggestions for what's typed, once they came.
    pub suggested: &'a [String],
}

/// The rows for `query`, at most [`MOST`].
pub fn rows(query: &Query, cx: &mut App) -> Vec<Row> {
    let typed = query.typed.trim();
    let mut rows = match (query.kind, typed.is_empty()) {
        (Kind::Browser, true) => browser_empty(query.here, cx),
        (Kind::Browser, false) => browser(query, typed, cx),
        (Kind::Chat, true) => chat_empty(query.here, cx),
        (Kind::Chat, false) => chat(query.here, typed, cx),
        (Kind::Terminal, true) => words(history::Kind::Command, "", query.here, MOST_EMPTY, cx),
        (Kind::Terminal, false) => terminal(query.here, typed, cx),
        // A worktree's name lists nothing: Enter takes it to the New Worktree dialog. Nor
        // does the agent desktop, of which there is one.
        (Kind::Device, _) => devices(query.here, typed, cx),
        (Kind::Worktree | Kind::Desktop, _) => Vec::new(),
    };
    rows.truncate(MOST);
    rows
}

/// A browser's rows with nothing typed: its tabs, then the top sites no tab shows.
fn browser_empty(here: &Here, cx: &mut App) -> Vec<Row> {
    let mut rows: Vec<Row> = (here.tabs_of(PaneKind::Browser))
        .take(MOST_EMPTY_TABS)
        .map(|tab| switch(tab, Vec::new(), Vec::new()))
        .collect();
    let top = history::get(cx).find(
        history::Kind::Page,
        "",
        &here.place,
        MOST_EMPTY,
        history::now(),
    );
    let pages = (top.into_iter())
        .filter(|found| here.tab_showing(&found.text).is_none())
        .map(|found| page(found, cx));
    rows.extend(pages);
    rows.truncate(MOST_EMPTY);
    rows
}

/// A browser's rows for `typed`.
fn browser(query: &Query, typed: &str, cx: &mut App) -> Vec<Row> {
    let (here, now) = (query.here, history::now());
    let mut rows = Vec::new();
    // The first row: the host completing what's typed, or the address typed, or a search.
    let address = match read_address(typed) {
        Some(Address::Page(url)) => Some(url),
        _ => None,
    };
    match (query.autofilled, &address) {
        (Some(host), _) => rows.push(origin(host, cx)),
        (None, Some(url)) => {
            rows.push(
                Row::new(
                    page_glyph(url, cx),
                    typed.to_owned(),
                    Act::Visit(url.clone()),
                )
                .hint("Visit"),
            );
        }
        (None, None) => {}
    }
    // An address without a scheme might be a search too, as Firefox offers.
    let has_scheme = typed.contains("://") || typed.starts_with("about:");
    if address.is_none() || !has_scheme {
        let search = Row::new(
            Glyph::Icon(IconName::Search),
            typed.to_owned(),
            Act::Search(typed.to_owned()),
        );
        rows.push(search.hint("Search with Google"));
    }
    let suggestions = if address.is_none() {
        searches(query, typed, cx)
    } else {
        Vec::new()
    };
    // A page the first row visits already isn't listed again, as Firefox drops what dupes its
    // first result; a tab showing it still is, to switch to.
    let first = rows.first().and_then(|row| match &row.act {
        Act::Visit(url) => Some(history::same_page_key(url)),
        _ => None,
    });
    let general: Vec<Row> = (pages(here, typed, now, cx).into_iter())
        .filter(|row| !matches!((&row.act, &first), (Act::Visit(url), Some(key)) if history::same_page_key(url) == *key))
        .collect();
    // Of the rows left, searches take two in three and pages the rest, as Firefox shares them out
    // with suggestions shown first; either takes what the other leaves.
    let room = MOST.saturating_sub(rows.len());
    let searches_room = (room * 2)
        .div_ceil(3)
        .max(room.saturating_sub(general.len()));
    let taken = suggestions.len().min(searches_room);
    rows.extend(suggestions.into_iter().take(taken));
    rows.extend(general.into_iter().take(room - taken));
    rows
}

/// The row of the host completing what's typed: its home page as the history has it, or the host.
fn origin(host: &str, cx: &mut App) -> Row {
    let home = (history::get(cx).of(history::Kind::Page))
        .find(|entry| history::same_page_key(&entry.text) == host)
        .map(|home| (home.text.clone(), home.title.trim().to_owned()));
    let (url, title) = match home {
        Some((url, title)) => (url, Some(title).filter(|title| !title.is_empty())),
        None => (
            address_to_url(host).unwrap_or_else(|| host.to_owned()),
            None,
        ),
    };
    let shown = title.clone().unwrap_or_else(|| host.to_owned());
    let mut row = Row::new(page_glyph(&url, cx), shown, Act::Visit(url));
    if title.is_some() {
        row.detail = host.to_owned().into();
    }
    row.hint("Visit")
}

/// The searches for what's typed: up to two made before, then the search engine's suggestions,
/// none the same as what's typed or each other, nor an address.
fn searches(query: &Query, typed: &str, cx: &App) -> Vec<Row> {
    let past = history::get(cx).find(
        history::Kind::Search,
        typed,
        &query.here.place,
        MOST_PAST_SEARCHES + 1,
        history::now(),
    );
    let mut seen: Vec<String> = vec![typed.to_lowercase()];
    let mut rows = Vec::new();
    for found in past {
        if seen.contains(&found.text.to_lowercase()) || rows.len() == MOST_PAST_SEARCHES {
            continue;
        }
        seen.push(found.text.to_lowercase());
        let mut row = Row::new(
            Glyph::Icon(IconName::Clock),
            found.title.clone(),
            Act::Search(found.text.clone()),
        );
        row.title_matched = found.title_matched;
        row.entry = Some((history::Kind::Search, found.text));
        rows.push(row);
    }
    for suggested in query.suggested {
        let lower = suggested.to_lowercase();
        if seen.contains(&lower) || is_address(suggested) {
            continue;
        }
        seen.push(lower);
        let mut row = Row::new(
            Glyph::Icon(IconName::Search),
            suggested.clone(),
            Act::Search(suggested.clone()),
        );
        row.title_matched = match_one(suggested, typed).unwrap_or_default();
        rows.push(row);
    }
    rows
}

/// The pages for what's typed: the history's, a page a tab shows becoming a switch to it, then the
/// tabs that match and the history hasn't.
fn pages(here: &Here, typed: &str, now: i64, cx: &mut App) -> Vec<Row> {
    let found = history::get(cx).find(history::Kind::Page, typed, &here.place, ASKED, now);
    let mut switched: Vec<u64> = Vec::new();
    let mut rows: Vec<Row> = (found.into_iter())
        .map(|found| match here.tab_showing(&found.text) {
            Some(tab) if !switched.contains(&tab.pane) => {
                switched.push(tab.pane);
                // The tab's own title may not be the one the history kept.
                matching_tab(tab, typed).unwrap_or_else(|| switch(tab, Vec::new(), Vec::new()))
            }
            _ => page(found, cx),
        })
        .collect();
    let unmatched = here
        .tabs_of(PaneKind::Browser)
        .filter(|tab| !switched.contains(&tab.pane));
    rows.extend(unmatched.filter_map(|tab| matching_tab(tab, typed)));
    rows
}

/// A switch to the browser tab `tab` if its title or address holds every word of `typed`.
fn matching_tab(tab: &OpenTab, typed: &str) -> Option<Row> {
    let shown = history::display_url(&tab.url);
    let mut marks = history::match_words(&[tab.title.as_ref(), shown.as_str()], typed)?;
    let detail = marks.pop().unwrap_or_default();
    Some(switch(tab, marks.pop().unwrap_or_default(), detail))
}

/// A page the history found.
fn page(found: Found, cx: &mut App) -> Row {
    Row {
        glyph: page_glyph(&found.text, cx),
        title: found.title.into(),
        title_matched: found.title_matched,
        detail: found.detail.into(),
        detail_matched: found.detail_matched,
        hint: None,
        act: Act::Visit(found.text.clone()),
        entry: Some((found.kind, found.text)),
    }
}

/// The icon of the page at `url`: its site's own, kept from a tab that showed it, or a globe.
fn page_glyph(url: &str, cx: &mut App) -> Glyph {
    history::icon(url, cx).map_or(Glyph::Icon(IconName::Globe), Glyph::Image)
}

/// The bytes of `field` the words of `typed` matched, if it holds every one.
fn match_one(field: &str, typed: &str) -> Option<Vec<Range<usize>>> {
    history::match_words(&[field], typed).and_then(|mut marks| marks.pop())
}

/// A switch to the open tab `tab`, the bytes of its title and address given matched.
fn switch(
    tab: &OpenTab,
    title_matched: Vec<Range<usize>>,
    detail_matched: Vec<Range<usize>>,
) -> Row {
    let glyph = match &tab.favicon {
        Some(icon) => Glyph::Image(Arc::clone(icon)),
        None => Glyph::Icon(tab.kind.icon()),
    };
    let mut row = Row::new(glyph, tab.title.clone(), Act::Switch(tab.pane)).hint("Switch to Tab");
    if tab.kind == PaneKind::Browser {
        row.detail = history::display_url(&tab.url).into();
    }
    (row.title_matched, row.detail_matched) = (title_matched, detail_matched);
    row
}

/// The tabs of `kind` whose titles hold every word of `typed`.
fn tabs_matching(here: &Here, kind: PaneKind, typed: &str) -> Vec<Row> {
    (here.tabs_of(kind))
        .filter_map(|tab| Some(switch(tab, match_one(&tab.title, typed)?, Vec::new())))
        .collect()
}

/// A chat's rows with nothing typed: its chats no tab shows, then what was said lately.
fn chat_empty(here: &Here, cx: &App) -> Vec<Row> {
    let mut rows: Vec<Row> = (here.chats.iter())
        .take(MOST_EMPTY_TABS)
        .map(|(id, title)| resume(id, title.clone(), Vec::new()))
        .collect();
    let room = MOST_EMPTY.saturating_sub(rows.len());
    rows.extend(words(history::Kind::Prompt, "", here, room, cx));
    rows
}

/// A chat's rows for `typed`: asking a new chat, then its tabs and chats that match, then what was
/// said before.
fn chat(here: &Here, typed: &str, cx: &App) -> Vec<Row> {
    let ask = Row::new(
        Glyph::Icon(IconName::MessageSquare),
        typed.to_owned(),
        Act::Ask(typed.to_owned()),
    );
    let mut rows = vec![ask.hint("Ask in a New Chat")];
    rows.extend(tabs_matching(here, PaneKind::Chat, typed));
    for (id, title) in &here.chats {
        if let Some(marks) = match_one(title, typed) {
            rows.push(resume(id, title.clone(), marks));
        }
    }
    let room = MOST.saturating_sub(rows.len());
    rows.extend(words(history::Kind::Prompt, typed, here, room, cx));
    rows
}

/// A chat no tab shows, which picking brings back.
fn resume(id: &ThreadId, title: SharedString, title_matched: Vec<Range<usize>>) -> Row {
    let mut row = Row::new(
        Glyph::Icon(IconName::MessagesSquare),
        title,
        Act::Resume(id.clone()),
    );
    row.title_matched = title_matched;
    row.hint("Open Chat")
}

/// A terminal's rows for `typed`: running it, then its tabs that match, then the commands run before.
fn terminal(here: &Here, typed: &str, cx: &App) -> Vec<Row> {
    let run = Row::new(
        Glyph::Icon(IconName::SquareTerminal),
        typed.to_owned(),
        Act::Run(typed.to_owned()),
    );
    let mut rows = vec![run.hint("Run in a New Terminal")];
    rows.extend(tabs_matching(here, PaneKind::Terminal, typed));
    let room = MOST.saturating_sub(rows.len());
    rows.extend(words(history::Kind::Command, typed, here, room, cx));
    rows
}

/// What was said, or run, before, for `typed`, but what's typed itself: picking one does it again.
fn words(kind: history::Kind, typed: &str, here: &Here, most: usize, cx: &App) -> Vec<Row> {
    let found = history::get(cx).find(kind, typed, &here.place, most + 1, history::now());
    (found.into_iter())
        .filter(|found| found.text.trim() != typed)
        .take(most)
        .map(|found| {
            let act = if kind == history::Kind::Prompt {
                Act::Ask(found.text.clone())
            } else {
                Act::Run(found.text.clone())
            };
            let mut row = Row::new(Glyph::Icon(IconName::Clock), found.title, act);
            row.title_matched = found.title_matched;
            row.entry = Some((kind, found.text));
            row
        })
        .collect()
}

/// A device's rows: its open tabs, then Koyo's simulators no tab shows, as its set last listed
/// them; those whose names hold every word typed.
fn devices(here: &Here, typed: &str, cx: &App) -> Vec<Row> {
    let mut rows = tabs_matching(here, PaneKind::Device, typed);
    let devices = crate::devices::devices(cx);
    let Some(Ok(simulators)) = &devices.read(cx).simulators else {
        return rows;
    };
    let open: Vec<&str> = (here.tabs_of(PaneKind::Device))
        .filter_map(|tab| tab.device.as_deref())
        .collect();
    for simulator in simulators.iter().filter(|simulator| !open.contains(&simulator.udid.as_str())) {
        let Some(matched) = match_one(&simulator.name, typed) else {
            continue;
        };
        let icon = if simulator.ipad { IconName::Tablet } else { IconName::Smartphone };
        let chosen = Chosen::from(simulator);
        let mut row = Row::new(Glyph::Icon(icon), simulator.name.clone(), Act::Device(chosen));
        row.title_matched = matched;
        row.detail = simulator.system.clone().into();
        rows.push(row);
    }
    rows.truncate(MOST);
    rows
}

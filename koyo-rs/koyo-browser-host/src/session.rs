//! A page's history and scroll as the app keeps them with the window
//! ([`koyo_browser_ipc::Session`]): the engine's list of the page's entries, read as it changes,
//! and where the page is scrolled to, which the renderer's watcher says as each scroll ends
//! (`renderer`). And a page brought back with them: its history put back with a stand-in
//! for each page of it (`stand_in`), the page loaded last, and scrolled once it has loaded.
//!
//! Only web pages (`http`, `https`) are kept in a history, the nearest [`KEPT`] each way: a
//! stand-in may replace itself with no other kind of page.
use cef::{
    Browser, CefString, ImplBrowser, ImplBrowserHost, ImplListValue, ImplNavigationEntry,
    ImplNavigationEntryVisitor, ImplProcessMessage, NavigationEntry, NavigationEntryVisitor,
    ProcessMessage, WrapNavigationEntryVisitor, rc::Rc, wrap_navigation_entry_visitor,
};
use koyo_browser_ipc::{PageState, PaneId, Scroll, Session, SessionEntry};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::os::raw::c_int;

pub(crate) mod renderer;
mod stand_in;

pub(crate) use stand_in::StandIns;

/// How many pages of a history are kept each way from the one shown.
const KEPT: usize = 20;

/// A page being brought back: where it is in being put in place, and where it was scrolled.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Restore {
    step: Step,
    /// Whether pages are ahead of it, so the last stand-in goes back to its own.
    ahead: bool,
    /// The page brought back, which the app is shown until it's in place.
    target: String,
    scroll: Scroll,
}

/// How far a page being brought back has got.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// A stand-in is loading; these load after it, in order: more stand-ins, and last the
    /// page itself when none is ahead of it.
    StandIns(VecDeque<String>),
    /// The page itself is loading.
    Page,
    /// The last stand-in went back to the page's own, which replaces itself with the page:
    /// how many loads have ended since, a stand-in's each.
    Back(u8),
}

/// What follows a load of a page being brought back.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Then {
    /// The next of its pages loads.
    Load(String),
    /// A stand-in's load ended on the way: the next one comes of itself.
    Wait,
    /// It's in place, and is scrolled back by the script, if it was scrolled.
    InPlace(Option<String>),
    /// Something else loaded: the person or an agent went somewhere else.
    Abandoned,
}

impl Restore {
    /// A load of the page's main frame ended, a stand-in's or not: what follows.
    pub(crate) fn loaded(&mut self, stand_in: bool) -> Then {
        match &mut self.step {
            Step::StandIns(_) if !stand_in => Then::Abandoned,
            Step::StandIns(then) => {
                let Some(next) = then.pop_front() else {
                    return Then::Abandoned;
                };
                if then.is_empty() {
                    self.step = if self.ahead { Step::Back(0) } else { Step::Page };
                }
                Then::Load(next)
            }
            Step::Page => Then::InPlace(self.scroll()),
            Step::Back(_) if !stand_in => Then::InPlace(self.scroll()),
            Step::Back(ended) => {
                *ended += 1;
                if *ended >= 3 {
                    Then::InPlace(None)
                } else {
                    Then::Wait
                }
            }
        }
    }

    /// The page brought back.
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    /// The script that scrolls the page back, once it has loaded; none at its top.
    fn scroll(&self) -> Option<String> {
        (!self.scroll.is_top()).then(|| scroll_script(self.scroll))
    }
}

/// Whether a page at `url` is one a history keeps.
fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Where a page at `url` brought back with `session` is first made, and what's left to bring
/// back once it's made: nothing for a page with no history or scroll to bring back. The
/// stand-ins it loads are issued the pane, in `issued`.
pub(crate) fn plan(url: &str, session: &Session, issued: &mut StandIns) -> (String, Option<Restore>) {
    let keep = |entries: &[SessionEntry]| -> Vec<SessionEntry> {
        entries.iter().filter(|e| is_web(&e.url)).cloned().collect()
    };
    let (mut back, mut forward) = (keep(&session.back), keep(&session.forward));
    back.drain(..back.len().saturating_sub(KEPT));
    forward.truncate(KEPT);
    let scroll = session.scroll;
    if !is_web(url) || (back.is_empty() && forward.is_empty()) {
        let restore = (!scroll.is_top()).then(|| Restore {
            step: Step::Page,
            ahead: false,
            target: url.to_owned(),
            scroll,
        });
        return (url.to_owned(), restore);
    }
    let mut then: VecDeque<String> = (back.iter().enumerate())
        .map(|(at, entry)| issued.issue(entry, at, 0))
        .collect();
    if forward.is_empty() {
        then.push_back(url.to_owned());
    } else {
        let shown = SessionEntry {
            url: url.to_owned(),
            title: String::new(),
        };
        then.push_back(issued.issue(&shown, back.len(), 0));
        let last = forward.len() - 1;
        for (ahead, entry) in forward.iter().enumerate() {
            let steps = if ahead == last { forward.len() } else { 0 };
            then.push_back(issued.issue(entry, back.len() + 1 + ahead, steps));
        }
    }
    let first = then.pop_front().unwrap_or_else(|| url.to_owned());
    let restore = Restore {
        step: Step::StandIns(then),
        ahead: !forward.is_empty(),
        target: url.to_owned(),
        scroll,
    };
    (first, Some(restore))
}

/// What the app is shown of a page whose state is `page`: while a stand-in the pane was
/// `issued` is shown, the page brought back, loading, as it's being put in place; otherwise
/// the page the stand-in stands for, which it replaces itself with once it has loaded. Any
/// other page, a `data:` one too, at its own address.
pub(crate) fn shown(page: &PageState, restore: Option<&Restore>, issued: &StandIns) -> PageState {
    let mut shown = page.clone();
    let Some(entry) = issued.entry_of(&page.url) else {
        return shown;
    };
    if let Some(restore) = restore {
        restore.target.clone_into(&mut shown.url);
        shown.title.clear();
        shown.loading = true;
    } else {
        (shown.url, shown.title) = (entry.url, entry.title);
    }
    shown
}

/// The watcher's process messages' name.
const SCROLLED: &str = "koyo-scrolled";

/// A process message from a page's renderer: `true` when it was the watcher's, which the
/// pane's session takes.
pub(crate) fn from_renderer(pane: PaneId, message: &ProcessMessage) -> bool {
    if CefString::from(&message.name()).to_string() != SCROLLED {
        return false;
    }
    let Some(args) = message.argument_list() else {
        return true;
    };
    let scroll = Scroll {
        x: args.int(0),
        y: args.int(1),
    };
    crate::host::session_changed(pane, |session| session.scroll = scroll);
    true
}

/// `pane`'s page is leaving for another document, which opens at its top.
pub(crate) fn left(pane: PaneId) {
    crate::host::session_changed(pane, |session| session.scroll = Scroll::default());
}

/// The engine's history of `pane`'s `browser` may have changed: the pages before and after
/// the one it shows, as the session has them.
pub(crate) fn history_changed(pane: PaneId, browser: &Browser) {
    let Some(host) = browser.host() else {
        return;
    };
    let listed = std::rc::Rc::new(RefCell::new(Listed::default()));
    let mut visitor = Listing::new(std::rc::Rc::clone(&listed));
    host.navigation_entries(Some(&mut visitor), 0);
    let Listed { entries, current } = listed.take();
    let Some(current) = current else {
        return;
    };
    let Some((back, forward)) = crate::host::stand_ins(pane, |issued| around(entries, current, issued)) else {
        return;
    };
    crate::host::session_changed(pane, |session| {
        session.back = back;
        session.forward = forward;
    });
}

/// The entries of a history before the one at `current`, oldest first, and after it,
/// nearest first: the web pages among them, the nearest [`KEPT`] each way, each stand-in the
/// pane was `issued` as the page it stands for.
fn around(
    mut entries: Vec<SessionEntry>,
    current: usize,
    issued: &StandIns,
) -> (Vec<SessionEntry>, Vec<SessionEntry>) {
    let kept = |entries: Vec<SessionEntry>| {
        entries
            .into_iter()
            .map(|entry| issued.entry_of(&entry.url).unwrap_or(entry))
            .filter(|entry| is_web(&entry.url))
    };
    let forward = kept(entries.split_off((current + 1).min(entries.len())))
        .take(KEPT)
        .collect();
    entries.truncate(current);
    let mut back: Vec<SessionEntry> = kept(entries).collect();
    back.drain(..back.len().saturating_sub(KEPT));
    (back, forward)
}

/// A session's history as the engine lists it.
#[derive(Default)]
struct Listed {
    entries: Vec<SessionEntry>,
    current: Option<usize>,
}

wrap_navigation_entry_visitor! {
    struct Listing {
        listed: std::rc::Rc<RefCell<Listed>>,
    }

    impl NavigationEntryVisitor {
        fn visit(
            &self,
            entry: Option<&mut NavigationEntry>,
            current: c_int,
            index: c_int,
            _total: c_int,
        ) -> c_int {
            let Some(entry) = entry else {
                return 1;
            };
            let mut listed = self.listed.borrow_mut();
            if current == 1 {
                listed.current = usize::try_from(index).ok();
            }
            listed.entries.push(SessionEntry {
                url: CefString::from(&entry.url()).to_string(),
                title: CefString::from(&entry.title()).to_string(),
            });
            1
        }
    }
}

/// `session`'s scroll, once the page it's of has loaded: what brings it back.
#[must_use]
pub(crate) fn scroll_script(scroll: Scroll) -> String {
    format!("({RESTORE_SCROLL})({}, {});", scroll.x, scroll.y)
}

/// Scrolls the page to `x`, `y`, and again as its document grows, until it's there, the
/// person scrolls or presses a key, or ten seconds have gone: a page that lays out after its
/// load reaches the place too, and one the person moves is left where they put it.
const RESTORE_SCROLL: &str = "(x, y) => { \
    const until = performance.now() + 10000; \
    const watched = document.documentElement; \
    let resized = null; \
    const stop = () => { if (resized) resized.disconnect(); \
        for (const name of ['wheel', 'touchstart', 'keydown', 'mousedown']) \
            removeEventListener(name, stop, true); }; \
    const go = () => { \
        if (performance.now() > until) { stop(); return; } \
        scrollTo(x, y); \
        if (Math.abs(scrollX - x) < 1 && Math.abs(scrollY - y) < 1) stop(); }; \
    for (const name of ['wheel', 'touchstart', 'keydown', 'mousedown']) \
        addEventListener(name, stop, { capture: true, passive: true }); \
    if (watched && typeof ResizeObserver === 'function') { \
        resized = new ResizeObserver(go); resized.observe(watched); } \
    go(); }";

#[cfg(test)]
mod tests;

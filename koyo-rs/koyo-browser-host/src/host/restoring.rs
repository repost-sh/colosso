//! A pane's history and scroll, as the app keeps them (`crate::session`): told to the app as
//! they change, and brought back as a page brought back opens. Its stand-ins load one after
//! another as each before it has loaded, the app shown the page brought back meanwhile; and
//! once that page has loaded it's scrolled back. The person going somewhere else stops it.
use super::queue::{Queued, enqueue};
use super::{browser_of, send, with_pane};
use cef::{CefString, ImplBrowser, ImplFrame};
use crate::session::Then;
use koyo_browser_ipc::{HostEvent, PaneId, Session};

/// Where `pane`'s page, at `url` and brought back with `session`, is first made; the rest of
/// what's brought back follows as it loads.
pub(super) fn open_at(pane: PaneId, url: &str, session: Session) -> String {
    with_pane(pane, |p| {
        let (first, restore) = crate::session::plan(url, &session, &mut p.stand_ins);
        p.restore = restore;
        p.session = session;
        first
    })
    .unwrap_or_else(|| url.to_owned())
}

/// What `read` makes of the stand-ins issued for `pane`'s history, while it's open.
pub(crate) fn stand_ins<R>(pane: PaneId, read: impl FnOnce(&crate::session::StandIns) -> R) -> Option<R> {
    with_pane(pane, |p| read(&p.stand_ins))
}

/// `pane`'s main frame finished loading: the next page of what's brought back loads, once
/// this callback is over; or, the page itself in place, it's scrolled back, and the app is
/// shown it as it is; or, something else having loaded, nothing more is brought back.
pub(crate) fn loaded(pane: PaneId) {
    let url = browser_of(pane)
        .and_then(|browser| browser.main_frame())
        .map(|frame| CefString::from(&frame.url()).to_string())
        .unwrap_or_default();
    let then = with_pane(pane, |p| {
        let stand_in = p.stand_ins.has(&url);
        let then = p.restore.as_mut()?.loaded(stand_in);
        if matches!(then, Then::InPlace(_) | Then::Abandoned) {
            p.restore = None;
        }
        Some(then)
    })
    .flatten();
    match then {
        Some(Then::Load(next)) => enqueue(Queued::Restoring { pane, url: next }),
        Some(Then::InPlace(scroll)) => {
            let frame = browser_of(pane).and_then(|browser| browser.main_frame());
            if let (Some(script), Some(frame)) = (scroll, frame) {
                frame.execute_java_script(Some(&script.as_str().into()), None, 0);
            }
            told_as_it_is(pane);
        }
        Some(Then::Abandoned) => told_as_it_is(pane),
        Some(Then::Wait) | None => {}
    }
}

/// What's brought back to `pane` is done with: the app is told of its page as it is, and of
/// its history.
fn told_as_it_is(pane: PaneId) {
    super::page_changed(pane, |_| {});
    if let Some(browser) = browser_of(pane) {
        crate::session::history_changed(pane, &browser);
    }
}

/// Loads `url`, the next page of what's brought back to `pane`, unless the person went
/// somewhere else meanwhile.
pub(super) fn load(pane: PaneId, url: &str) {
    if with_pane(pane, |p| p.restore.is_some()) != Some(true) {
        return;
    }
    if let Some(frame) = browser_of(pane).and_then(|browser| browser.main_frame()) {
        frame.load_url(Some(&url.into()));
    }
}

/// The person went somewhere else in `pane`'s page, or its renderer went away: what's still
/// to be brought back isn't. The page it was bringing back, if it was.
pub(crate) fn abandon(pane: PaneId) -> Option<String> {
    let restore = with_pane(pane, |p| p.restore.take()).flatten()?;
    told_as_it_is(pane);
    Some(restore.target().to_owned())
}

/// Makes `change` to `pane`'s session, telling the app if it changed: not while it's being
/// brought back, as the app has it as it was.
pub(crate) fn session_changed(pane: PaneId, change: impl FnOnce(&mut Session)) {
    let session = with_pane(pane, |p| {
        if p.restore.is_some() {
            return None;
        }
        let before = p.session.clone();
        change(&mut p.session);
        (p.session != before).then(|| p.session.clone())
    })
    .flatten();
    if let Some(session) = session {
        send(&HostEvent::Session { pane, session });
    }
}

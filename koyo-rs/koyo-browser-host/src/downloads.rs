//! A page's downloads (`docs/browser.md` 16). Chromium asks where each goes; the app decides,
//! by the person's settings and whose page it is, and answers, the host keeping CEF's callback
//! until then. Then the app hears how each stands, and the host keeps the way to cancel it.
//!
//! CEF tells a client of a download only while the page that started it is open. So a page
//! the app closes while its downloads come in stays open, out of sight, muted and on a blank
//! page, and goes once the last ends, as Chrome keeps a closed tab's downloads. A page that
//! closes itself (`window.close()`) can't be kept: Chrome closes its browser before CEF tells of
//! it, and its downloads end with it, nothing of them kept, which the app is told.
//! Everything here runs on CEF's UI thread.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::os::raw::c_int;

use cef::{
    BeforeDownloadCallback, Browser, CefString, DownloadHandler, DownloadItem,
    DownloadItemCallback, ImplBeforeDownloadCallback, ImplBrowser, ImplBrowserHost,
    ImplDownloadHandler, ImplDownloadItem, ImplDownloadItemCallback, ImplFrame,
    WrapDownloadHandler, rc::Rc, wrap_download_handler,
};
use koyo_browser_ipc::{Download, DownloadAct, DownloadState, HostEvent, PaneId};

use crate::host;

/// A download: the page that started it, and the engine's number for it.
type Key = (PaneId, u32);

thread_local! {
    /// Downloads asked about and not answered yet, with where each comes from: a callback
    /// dropped unanswered cancels its download.
    static ASKED: RefCell<HashMap<Key, (BeforeDownloadCallback, String)>> = RefCell::new(HashMap::new());
    /// Downloads coming in, with the way to cancel each and how it last stood.
    static GOING: RefCell<HashMap<Key, (DownloadItemCallback, Download)>> = RefCell::new(HashMap::new());
    /// Pages the app closed that stay open for their downloads.
    static KEPT: RefCell<HashSet<PaneId>> = RefCell::new(HashSet::new());
}

/// The handler of `pane`'s downloads.
pub(crate) fn handler(pane: PaneId) -> DownloadHandler {
    PaneDownloads::new(pane)
}

wrap_download_handler! {
    struct PaneDownloads {
        pane: PaneId,
    }

    impl DownloadHandler {
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> c_int {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> c_int {
            let (Some(item), Some(callback)) = (download_item, callback) else {
                return 0;
            };
            let (download, url) = (item.id(), text(&item.url()));
            ASKED.with(|asked| asked.borrow_mut().insert((self.pane, download), (callback.clone(), url.clone())));
            host::send(&HostEvent::DownloadAsked {
                pane: self.pane,
                download,
                name: suggested_name.map(ToString::to_string).unwrap_or_default(),
                url,
            });
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(download) = download_item.and_then(|item| Seen::of(item).download()) else {
                return;
            };
            let key = (self.pane, download.id);
            let going = download.state == DownloadState::Going;
            GOING.with(|going_on| {
                let mut going_on = going_on.borrow_mut();
                match callback.filter(|_| going) {
                    Some(callback) => going_on.insert(key, (callback.clone(), download.clone())),
                    None => going_on.remove(&key),
                }
            });
            host::send(&HostEvent::Download { pane: self.pane, download });
            if !going {
                ended(self.pane);
            }
        }
    }
}

/// What the app has done with `pane`'s download `download`: given it a place, or canceled it.
pub(crate) fn act(pane: PaneId, download: u32, act: DownloadAct) {
    let key = (pane, download);
    let asked = ASKED.with(|asked| asked.borrow_mut().remove(&key));
    match act {
        DownloadAct::SaveTo { path } => {
            if let Some((asked, _)) = asked {
                asked.cont(Some(&CefString::from(path.as_str())), 0);
            }
        }
        DownloadAct::Cancel => {
            drop(asked);
            let going = GOING.with(|going| going.borrow().get(&key).map(|(going, _)| going.clone()));
            if let Some(going) = going {
                going.cancel();
            }
            ended(pane);
        }
    }
}

/// Whether `pane`'s page, which the app closed, stays open for downloads still to come in:
/// out of sight, muted, and on a blank page, until the last ends.
pub(crate) fn keep_open(pane: PaneId, browser: &Browser) -> bool {
    if !has_downloads(pane) {
        return false;
    }
    KEPT.with(|kept| kept.borrow_mut().insert(pane));
    crate::native::sight(pane, false, false);
    if let Some(host) = browser.host() {
        host.set_audio_muted(1);
    }
    if let Some(frame) = browser.main_frame() {
        frame.load_url(Some(&CefString::from("about:blank")));
    }
    true
}

/// A download of `pane`'s ended, or went: a page kept open for its downloads closes after the
/// last.
fn ended(pane: PaneId) {
    let last = !has_downloads(pane) && KEPT.with(|kept| kept.borrow_mut().remove(&pane));
    if last {
        host::close_later(pane);
    }
}

fn has_downloads(pane: PaneId) -> bool {
    let of_pane = |key: &Key| key.0 == pane;
    ASKED.with(|asked| asked.borrow().keys().any(of_pane))
        || GOING.with(|going| going.borrow().keys().any(of_pane))
}

/// `pane`'s browser is gone, and its downloads still asked about or coming in with it: the
/// app is told each ended there.
pub(crate) fn pane_closed(pane: PaneId) {
    let mut gone = Vec::new();
    ASKED.with(|asked| {
        asked.borrow_mut().retain(|&(of, id), (_, url)| {
            let keep = of != pane;
            if !keep {
                gone.push(Download { id, url: std::mem::take(url), path: String::new(), received: 0, total: None, state: DownloadState::Going });
            }
            keep
        });
    });
    GOING.with(|going| {
        going.borrow_mut().retain(|key, (_, download)| {
            let keep = key.0 != pane;
            if !keep {
                gone.push(download.clone());
            }
            keep
        });
    });
    KEPT.with(|kept| kept.borrow_mut().remove(&pane));
    for download in gone {
        host::send(&HostEvent::Download { pane, download: closed_with_its_page(download) });
    }
}

/// How `download` ended, its page closed before it did.
fn closed_with_its_page(download: Download) -> Download {
    let reason = "The page that started it closed before it finished.".to_owned();
    Download { state: DownloadState::Failed { reason }, ..download }
}

/// The host is shutting down; CEF waits for every callback to go.
pub(crate) fn clear() {
    ASKED.with(|asked| asked.borrow_mut().clear());
    GOING.with(|going| going.borrow_mut().clear());
    KEPT.with(|kept| kept.borrow_mut().clear());
}

fn text(value: &cef::CefStringUserfree) -> String {
    CefString::from(value).to_string()
}

/// What the host reads of a download as CEF says it stands.
#[derive(Default)]
struct Seen {
    id: u32,
    url: String,
    path: String,
    received: i64,
    total: i64,
    complete: bool,
    canceled: bool,
    interrupted: bool,
    reason: i32,
}

impl Seen {
    fn of(item: &DownloadItem) -> Self {
        Seen {
            id: item.id(),
            url: text(&item.url()),
            path: text(&item.full_path()),
            received: item.received_bytes(),
            total: item.total_bytes(),
            complete: item.is_complete() != 0,
            canceled: item.is_canceled() != 0,
            interrupted: item.is_interrupted() != 0,
            reason: cef::sys::cef_download_interrupt_reason_t::from(item.interrupt_reason()) as i32,
        }
    }

    /// How it stands for the app: none while it's coming with no place yet, which the app is
    /// asked for first.
    fn download(self) -> Option<Download> {
        let state = if self.complete {
            DownloadState::Done
        } else if self.canceled || (self.interrupted && self.reason == USER_CANCELED) {
            DownloadState::Canceled
        } else if self.interrupted {
            DownloadState::Failed {
                reason: why(self.reason).into(),
            }
        } else {
            DownloadState::Going
        };
        if self.path.is_empty() && state == DownloadState::Going {
            return None;
        }
        let received = u64::try_from(self.received).unwrap_or(0);
        let total = u64::try_from(self.total).ok().filter(|&total| total > 0);
        Some(Download {
            id: self.id,
            url: self.url,
            path: self.path,
            received,
            total,
            state,
        })
    }
}

/// Chromium's reason for a download the person canceled.
const USER_CANCELED: i32 = 40;

/// Why a download stopped short, for the person, by Chromium's reason: its file's, the
/// network's or the server's, or the engine's going.
fn why(reason: i32) -> &'static str {
    match reason {
        2 => "Koyo may not write to that folder.",
        3 | 6 => "The disk is full.",
        5 => "The file's name is too long.",
        7 | 11 | 12 => "The file was blocked as unsafe.",
        1..=19 => "The file couldn't be written.",
        21 => "The network timed out.",
        20..=29 => "The network went away.",
        34 => "The server asks to sign in first.",
        35 => "The server's certificate isn't valid.",
        36 => "The server refused it.",
        30..=39 => "The server stopped sending it.",
        41 => "Koyo quit before it finished.",
        50 => "The browser engine stopped.",
        _ => "It stopped.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coming(path: &str, received: i64, total: i64) -> Seen {
        Seen {
            id: 3,
            url: "https://example.com/a.zip".into(),
            path: path.into(),
            received,
            total,
            ..Seen::default()
        }
    }

    #[test]
    fn a_download_with_no_place_yet_is_the_apps_question_not_news() {
        assert_eq!(coming("", 0, 0).download(), None);
        let canceled = Seen {
            canceled: true,
            ..coming("", 0, 0)
        };
        assert_eq!(
            canceled.download().map(|download| download.state),
            Some(DownloadState::Canceled),
            "one canceled before it had a place still ends"
        );
    }

    #[test]
    fn bytes_come_with_a_total_only_once_the_server_says_one() {
        let unknown = coming("/d/a.zip", 2048, -1).download().unwrap();
        assert_eq!((unknown.received, unknown.total, unknown.state), (2048, None, DownloadState::Going));
        let known = coming("/d/a.zip", 2048, 0).download().unwrap();
        assert_eq!(known.total, None, "a total of none is no total");
        let sized = coming("/d/a.zip", 2048, 8192).download().unwrap();
        assert_eq!((sized.received, sized.total), (2048, Some(8192)));
        assert_eq!((sized.path.as_str(), sized.id), ("/d/a.zip", 3));
    }

    #[test]
    fn a_download_whose_page_closed_ended_there_as_far_as_it_came() {
        let going = coming("/d/a.zip", 2048, 8192).download().unwrap();
        let ended = closed_with_its_page(going.clone());
        assert_eq!((ended.received, ended.total, &ended.path), (2048, Some(8192), &going.path));
        assert_eq!(
            ended.state,
            DownloadState::Failed { reason: "The page that started it closed before it finished.".into() }
        );
    }

    #[test]
    fn how_a_download_ended_is_said_for_the_person() {
        let state = |seen: Seen| seen.download().unwrap().state;
        let done = Seen { complete: true, ..coming("/d/a.zip", 8192, 8192) };
        assert_eq!(state(done), DownloadState::Done);
        let dropped = Seen { interrupted: true, reason: 22, ..coming("/d/a.zip", 10, 8192) };
        assert_eq!(
            state(dropped),
            DownloadState::Failed { reason: "The network went away.".into() }
        );
        let full = Seen { interrupted: true, reason: 3, ..coming("/d/a.zip", 10, 8192) };
        assert_eq!(state(full), DownloadState::Failed { reason: "The disk is full.".into() });
        let by_person = Seen { interrupted: true, reason: USER_CANCELED, ..coming("/d/a.zip", 10, 0) };
        assert_eq!(state(by_person), DownloadState::Canceled);
        assert_eq!(why(999), "It stopped.");
    }
}

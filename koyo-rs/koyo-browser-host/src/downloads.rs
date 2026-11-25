//! A page's downloads (`docs/browser.md` 16). Chromium asks where each goes; the app decides,
//! by the person's settings and whose page it is, and answers, the host keeping CEF's callback
//! until then. Then the app hears how each stands, and the host keeps the way to cancel it.
//!
//! A download outlives the page that asked for it, as Chrome's outlive their tab: the engine
//! asks this handler where it goes, and goes on telling it how it stands, after its page has
//! closed (`script/cef-patches/download-outlives-its-page.patch`,
//! `request-outlives-its-page.patch`), so nothing here is tied to the page being open.
//! Everything here runs on CEF's UI thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use cef::{
    BeforeDownloadCallback, Browser, CefString, DownloadHandler, DownloadItem,
    DownloadItemCallback, ImplBeforeDownloadCallback, ImplDownloadHandler,
    ImplDownloadItem, ImplDownloadItemCallback, WrapDownloadHandler, rc::Rc, wrap_download_handler,
};
use koyo_browser_ipc::{Download, DownloadAct, DownloadState, HostEvent, PaneId};

use crate::host;

/// A download as the engine knows it: its handler's number, and its own in the profile of the
/// handler's browser, which another profile's download has too.
type Item = (u64, u32);

/// A download that hasn't ended.
struct Coming {
    item: Item,
    stage: Stage,
}

/// Where a download that hasn't ended is.
enum Stage {
    /// The app is asked where it goes: the answer, which cancels it when it's dropped ungiven.
    Asked(BeforeDownloadCallback),
    /// It goes to the file at `path`, with the way to cancel it once the engine has said how it
    /// stands.
    Going {
        path: String,
        cancel: Option<DownloadItemCallback>,
    },
    /// It's to end.
    Canceled,
}

thread_local! {
    /// The downloads that haven't ended, by the number the app knows each by.
    static COMING: RefCell<HashMap<u32, Coming>> = RefCell::new(HashMap::new());
}

fn with<R>(f: impl FnOnce(&mut HashMap<u32, Coming>) -> R) -> R {
    COMING.with(|coming| f(&mut coming.borrow_mut()))
}

/// The handler of the downloads a browser of `pane`'s asks for: one for each browser.
pub(crate) fn handler(pane: PaneId) -> DownloadHandler {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PaneDownloads::new(pane, NEXT.fetch_add(1, Ordering::Relaxed))
}

wrap_download_handler! {
    struct PaneDownloads {
        pane: PaneId,
        number: u64,
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
            static NEXT: AtomicU32 = AtomicU32::new(1);
            let (Some(item), Some(callback)) = (download_item, callback) else {
                return 0;
            };
            if host::shutting_down() {
                return 1;
            }
            let key = (self.number, item.id());
            if let Some(download) = number_of(key) {
                asked_again(download, callback);
                return 1;
            }
            let download = NEXT.fetch_add(1, Ordering::Relaxed);
            let asked = Coming { item: key, stage: Stage::Asked(callback.clone()) };
            with(|coming| coming.insert(download, asked));
            host::send(&HostEvent::DownloadAsked {
                pane: self.pane,
                download,
                name: suggested_name.map(ToString::to_string).unwrap_or_default(),
                url: text(&item.url()),
            });
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = download_item else {
                return;
            };
            let Some(id) = number_of((self.number, item.id())) else {
                return;
            };
            let Some(download) = Seen::of(id, item).download() else {
                return;
            };
            if download.state != DownloadState::Going {
                drop(with(|coming| coming.remove(&id)));
                host::send(&HostEvent::Download { download });
                host::quit_when_closed();
                return;
            }
            let Some(callback) = callback else {
                return;
            };
            let canceled = with(|coming| match coming.get_mut(&id).map(|one| &mut one.stage) {
                Some(Stage::Going { cancel, .. }) => {
                    *cancel = Some(callback.clone());
                    false
                }
                Some(Stage::Canceled) => true,
                Some(Stage::Asked(_)) | None => false,
            });
            if canceled {
                callback.cancel();
            } else {
                host::send(&HostEvent::Download { download });
            }
        }
    }
}

/// The number the app knows the download `item` by, while it hasn't ended.
fn number_of(item: Item) -> Option<u32> {
    with(|coming| coming.iter().find(|(_, one)| one.item == item).map(|(number, _)| *number))
}

/// The engine asks again where `download` goes, as it does for each attempt of one the server
/// dropped and it resumes: where the app said. One canceled meanwhile ends, unanswered.
fn asked_again(download: u32, callback: &BeforeDownloadCallback) {
    let path = with(|coming| match &coming.get(&download)?.stage {
        Stage::Going { path, .. } => Some(path.clone()),
        Stage::Asked(_) | Stage::Canceled => None,
    });
    if let Some(path) = path {
        callback.cont(Some(&CefString::from(path.as_str())), 0);
    }
}

/// What the app has done with the download `download`: given it a place, or canceled it.
pub(crate) fn act(download: u32, act: DownloadAct) {
    match act {
        DownloadAct::SaveTo { path } => save_to(download, path),
        DownloadAct::Cancel => cancel(download),
    }
}

/// The download `download`, asked about, goes to `path`.
fn save_to(download: u32, path: String) {
    let to = CefString::from(path.as_str());
    let asked = with(|coming| {
        let stage = &mut coming.get_mut(&download)?.stage;
        match std::mem::replace(stage, Stage::Going { path, cancel: None }) {
            Stage::Asked(asked) => Some(asked),
            was @ (Stage::Going { .. } | Stage::Canceled) => {
                *stage = was;
                None
            }
        }
    });
    if let Some(asked) = asked {
        asked.cont(Some(&to), 0);
    }
}

/// The download `download` ends: at once, or once the engine says how it stands.
fn cancel(download: u32) {
    let was = with(|coming| {
        let stage = &mut coming.get_mut(&download)?.stage;
        Some(std::mem::replace(stage, Stage::Canceled))
    });
    if let Some(Stage::Going { cancel: Some(cancel), .. }) = was {
        cancel.cancel();
    }
}

/// Whether no download is still to end.
pub(crate) fn all_ended() -> bool {
    with(|coming| coming.is_empty())
}

/// The host is shutting down: what's still coming in ends with it, as Chrome's do when it
/// quits, and the host waits for it to (`host::all_closed`).
pub(crate) fn cancel_all() {
    for download in with(|coming| coming.keys().copied().collect::<Vec<_>>()) {
        cancel(download);
    }
}

/// The host is shutting down; CEF waits for every callback to go.
pub(crate) fn clear() {
    drop(with(std::mem::take));
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
    fn of(id: u32, item: &DownloadItem) -> Self {
        Seen {
            id,
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

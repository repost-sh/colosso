//! What pages download, and the pages saved as PDFs (`docs/browser.md` 16): one list for the
//! app, oldest first, kept while it runs. Each download asks where it goes. It goes to the
//! Downloads folder under a name nothing there has, `report (1).pdf` beside `report.pdf` as
//! Chrome names them, or where the person picks in the system's save panel when Settings →
//! Browser says to ask: never for a page an agent drives, nor one whose tab is closed. The
//! downloads button in the window's top bar opens the list (`button.rs`, `panel.rs`).

use std::path::{Path, PathBuf};

use gpui_kit::{App, AppContext as _, Context, Entity, Global};
use koyo_browser_ipc::{self as ipc, AppRequest, DownloadAct, DownloadState, PaneId, Print};

use super::control::{Leased as _, Mode};
use super::host::Browsers;
use super::view::BrowserView;
use crate::blocking::{self, Blocking};

mod button;
mod panel;
pub use button::{CameOrWent, DownloadsButton, ToggleDownloads};
#[cfg(test)]
mod tests;
#[cfg(feature = "visual-check")]
mod visual;
#[cfg(feature = "visual-check")]
pub use visual::run as check;

/// Which download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Key {
    /// A page's: the host's number for it.
    Engine(u32),
    /// A page saved as a PDF, numbered by the app.
    Pdf(u64),
}

/// Where a download is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    /// Its place is being found: a free name in the Downloads folder, or the person's pick.
    Placing,
    Going,
    Done,
    Canceled,
    Failed(String),
}

impl Stage {
    pub(super) fn ended(&self) -> bool {
        matches!(self, Stage::Done | Stage::Canceled | Stage::Failed(_))
    }
}

/// Who the page that downloads is with: the person, an agent, or nobody, its tab gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum At {
    Person,
    Agent(String),
    Nobody,
}

/// Who `page` is with.
pub(super) fn at(page: Option<&BrowserView>) -> At {
    match page.map(|page| page.lease.control.agent()) {
        Some(Some(agent)) => At::Agent(agent.label.clone()),
        Some(None) => At::Person,
        None => At::Nobody,
    }
}

/// A download in the list.
#[derive(Clone)]
pub(super) struct Entry {
    pub(super) key: Key,
    /// Its file's name.
    pub(super) name: String,
    /// Its file, once it has a place.
    pub(super) path: Option<PathBuf>,
    pub(super) received: u64,
    pub(super) total: Option<u64>,
    pub(super) stage: Stage,
    /// The agent whose page downloaded it.
    pub(super) by: Option<String>,
}

#[derive(Default)]
pub(super) struct Downloads {
    list: Vec<Entry>,
    /// The PDFs saved so far, which number the next.
    pdfs: u64,
    /// How many downloads have started this run, which the button marks the arrival of.
    arrivals: u64,
    /// Where downloads go without asking, in a test: a folder of its own.
    #[cfg(test)]
    pub(super) folder: Option<PathBuf>,
}

struct GlobalDownloads(Entity<Downloads>);
impl Global for GlobalDownloads {}

impl Downloads {
    /// The app's downloads, made on first use.
    pub(super) fn global(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<GlobalDownloads>() {
            return global.0.clone();
        }
        let downloads = cx.new(|_| Downloads::default());
        cx.set_global(GlobalDownloads(downloads.clone()));
        downloads
    }

    /// The downloads of this run, oldest first; none before the first.
    pub(super) fn listed(cx: &App) -> &[Entry] {
        cx.try_global::<GlobalDownloads>()
            .map_or(&[], |global| global.0.read(cx).list.as_slice())
    }

    /// How many downloads have started this run.
    pub(super) fn arrivals(cx: &App) -> u64 {
        cx.try_global::<GlobalDownloads>().map_or(0, |global| global.0.read(cx).arrivals)
    }

    fn entry(&mut self, key: Key) -> Option<&mut Entry> {
        self.list.iter_mut().find(|entry| entry.key == key)
    }

    /// Where downloads go unless the person picks a place: the folder Settings names, else
    /// their Downloads folder.
    fn folder(&self, cx: &App) -> PathBuf {
        #[cfg(test)]
        if let Some(folder) = &self.folder {
            return folder.clone();
        }
        (crate::settings::get(cx).downloads_dir.clone())
            .or_else(dirs::download_dir)
            .or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
            .unwrap_or_default()
    }

    /// Where downloads under way go: no other may go there.
    fn taken(&self) -> Vec<PathBuf> {
        (self.list.iter())
            .filter(|entry| !entry.stage.ended())
            .filter_map(|entry| entry.path.clone())
            .collect()
    }

    /// A page asks where its download `id` goes, `name` the name it gave it, the page
    /// being `at` someone's. It goes to the Downloads folder, under a name free there, or
    /// where the person picks, when they're at the page and Settings asks for it.
    pub(super) fn asked(&mut self, id: u32, name: &str, at: At, cx: &mut Context<Self>) {
        let key = Key::Engine(id);
        let name = file_name(name);
        let ask = at == At::Person && crate::settings::get(cx).downloads_ask;
        let by = match at {
            At::Agent(agent) => Some(agent),
            At::Person | At::Nobody => None,
        };
        self.list.push(Entry {
            key,
            name: name.clone(),
            path: None,
            received: 0,
            total: None,
            stage: Stage::Placing,
            by,
        });
        self.arrivals += 1;
        cx.notify();
        if ask {
            let panel = cx.prompt_for_new_path(&self.folder(cx), Some(&name));
            cx.spawn(async move |this, cx| {
                let picked = panel.await.ok().and_then(Result::ok).flatten();
                let _ = this.update(cx, |this, cx| this.place(key, picked, cx));
            })
            .detach();
        } else {
            self.find_place(key, name, cx);
        }
    }

    /// Finds a free name for `key` in the Downloads folder, off the main thread.
    fn find_place(&mut self, key: Key, name: String, cx: &mut Context<Self>) {
        let (folder, taken) = (self.folder(cx), self.taken());
        let found = blocking::spawn(cx.background_executor(), move |may| {
            free_path(&folder, &name, &taken, may)
        });
        cx.spawn(async move |this, cx| {
            let path = found.await;
            let _ = this.update(cx, |this, cx| {
                if this.taken().contains(&path) {
                    let name = this.entry(key).map(|entry| entry.name.clone()).unwrap_or_default();
                    this.find_place(key, name, cx);
                } else {
                    this.place(key, Some(path), cx);
                }
            });
        })
        .detach();
    }

    /// `key` goes to `path`, or, with none, is canceled: unless it was canceled meanwhile.
    fn place(&mut self, key: Key, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let Key::Engine(id) = key else {
            return;
        };
        let Some(entry) = self.entry(key).filter(|entry| entry.stage == Stage::Placing) else {
            return;
        };
        let act = if let Some(path) = path {
            entry.name = shown_name(&path, &entry.name);
            let act = DownloadAct::SaveTo { path: path.to_string_lossy().into_owned() };
            entry.path = Some(path);
            entry.stage = Stage::Going;
            act
        } else {
            entry.stage = Stage::Canceled;
            DownloadAct::Cancel
        };
        send(AppRequest::Download { download: id, act }, cx);
        cx.notify();
    }

    /// How a download stands, as the host says: one canceled stays so, however late the host
    /// heard.
    pub(super) fn updated(&mut self, download: ipc::Download, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(Key::Engine(download.id)) else {
            return;
        };
        if entry.stage == Stage::Canceled && download.state == DownloadState::Going {
            return;
        }
        if !download.path.is_empty() {
            let path = PathBuf::from(download.path);
            entry.name = shown_name(&path, &entry.name);
            entry.path = Some(path);
        }
        entry.received = download.received;
        entry.total = download.total;
        entry.stage = match download.state {
            DownloadState::Going => Stage::Going,
            DownloadState::Done => Stage::Done,
            DownloadState::Canceled => Stage::Canceled,
            DownloadState::Failed { reason } => Stage::Failed(reason),
        };
        cx.notify();
    }

    /// Cancels `key`, unless it has ended: a PDF can't be.
    pub(super) fn cancel(&mut self, key: Key, cx: &mut Context<Self>) {
        let Key::Engine(id) = key else {
            return;
        };
        let Some(entry) = self.entry(key).filter(|entry| !entry.stage.ended()) else {
            return;
        };
        entry.stage = Stage::Canceled;
        send(AppRequest::Download { download: id, act: DownloadAct::Cancel }, cx);
        cx.notify();
    }

    /// Takes the downloads that ended off the list.
    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        self.list.retain(|entry| !entry.stage.ended());
        cx.notify();
    }

    /// Saves `pane`'s page, titled `title`, as a PDF where the person picks in the system's
    /// save panel, the Downloads folder first.
    pub(super) fn save_pdf(&mut self, pane: PaneId, title: &str, cx: &mut Context<Self>) {
        let panel = cx.prompt_for_new_path(&self.folder(cx), Some(&pdf_name(title)));
        cx.spawn(async move |this, cx| {
            let Some(path) = panel.await.ok().and_then(Result::ok).flatten() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.saving_pdf(pane, path, cx));
        })
        .detach();
    }

    /// `pane`'s page is saved as a PDF at `path`, the person's pick in the save panel, which
    /// the list shows until it's written. Picked with no extension, it gets `.pdf`, under a name
    /// nothing there has: the panel asked about replacing the name picked, not that one.
    pub(super) fn saving_pdf(&mut self, pane: PaneId, path: PathBuf, cx: &mut Context<Self>) {
        let (Some(folder), Some(name), None) = (path.parent(), path.file_name(), path.extension()) else {
            return self.pdf_at(pane, path, cx);
        };
        let (folder, taken) = (folder.to_owned(), self.taken());
        let name = format!("{}.pdf", name.to_string_lossy());
        let found = blocking::spawn(cx.background_executor(), move |may| {
            free_path(&folder, &name, &taken, may)
        });
        cx.spawn(async move |this, cx| {
            let path = found.await;
            let _ = this.update(cx, |this, cx| this.pdf_at(pane, path, cx));
        })
        .detach();
    }

    /// `pane`'s page is saved as a PDF at `path`, which the list shows until it's written.
    fn pdf_at(&mut self, pane: PaneId, path: PathBuf, cx: &mut Context<Self>) {
        self.pdfs += 1;
        let print = Print::Pdf { path: path.to_string_lossy().into_owned() };
        self.list.push(Entry {
            key: Key::Pdf(self.pdfs),
            name: shown_name(&path, "page.pdf"),
            path: Some(path),
            received: 0,
            total: None,
            stage: Stage::Going,
            by: None,
        });
        self.arrivals += 1;
        send(AppRequest::Print { pane, print }, cx);
        cx.notify();
    }

    /// The PDF being saved at `path` was written, or couldn't be.
    pub(super) fn pdf_saved(&mut self, path: &str, saved: bool, cx: &mut Context<Self>) {
        let saving = (self.list.iter_mut()).find(|entry| {
            matches!(entry.key, Key::Pdf(_))
                && entry.stage == Stage::Going
                && entry.path.as_deref() == Some(Path::new(path))
        });
        if let Some(entry) = saving {
            entry.stage = if saved {
                Stage::Done
            } else {
                Stage::Failed("The page couldn't be saved as a PDF.".into())
            };
            cx.notify();
        }
    }
}

impl BrowserView {
    /// Prints the page in Chrome's print preview, as its own `window.print()` does: the
    /// person's act on it. `DevTools` print nothing.
    pub(super) fn print(&mut self, cx: &mut Context<Self>) {
        if self.is_inspector() || !self.is_open(cx) {
            return;
        }
        self.person_acts(Mode::Implicit, cx);
        let pane = self.pane;
        self.send_to_page(AppRequest::Print { pane, print: Print::Preview }, cx);
    }

    /// Saves the page as a PDF where the person picks, as it would print.
    pub(super) fn save_as_pdf(&mut self, cx: &mut Context<Self>) {
        if self.is_inspector() || !self.is_open(cx) {
            return;
        }
        self.person_acts(Mode::Implicit, cx);
        let (pane, title) = (self.pane, self.title());
        Downloads::global(cx).update(cx, |downloads, cx| downloads.save_pdf(pane, &title, cx));
    }
}

/// Has the host do `request`, after the update that asks it: the browsers may be the ones
/// telling of the download.
fn send(request: AppRequest, cx: &mut App) {
    let browsers = Browsers::global(cx);
    cx.defer(move |cx| browsers.update(cx, |browsers, _| browsers.send(request)));
}

/// The name a page gave its file, as one to save under: its last part, `download` for none.
fn file_name(name: &str) -> String {
    let last = Path::new(name.trim()).file_name().and_then(|last| last.to_str());
    match last {
        Some(last) if !last.trim().is_empty() => last.to_owned(),
        _ => "download".to_owned(),
    }
}

/// The name a download's list row shows for its file at `path`; `fallback` for one with none.
fn shown_name(path: &Path, fallback: &str) -> String {
    (path.file_name()).map_or_else(|| fallback.to_owned(), |name| name.to_string_lossy().into_owned())
}

/// The name a page saved as a PDF gets: its title, with no `/` or `:`, which a Mac's file
/// names can't have.
fn pdf_name(title: &str) -> String {
    let title: String = title.trim().chars().map(|c| if matches!(c, '/' | ':') { '-' } else { c }).collect();
    let title = if title.is_empty() { "Page" } else { title.as_str() };
    format!("{title}.pdf")
}

/// `name` as the `n`th of its name: itself first, then `name (1)`, `name (2)`, the number
/// before its extension, `.tar.gz` kept whole.
fn numbered(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_owned();
    }
    let lower = name.to_ascii_lowercase();
    let at = (lower.rfind(".tar.").filter(|&at| at > 0))
        .or_else(|| name.rfind('.').filter(|&at| at > 0));
    match at {
        Some(at) => format!("{} ({n}){}", &name[..at], &name[at..]),
        None => format!("{name} ({n})"),
    }
}

/// The first of `name`'s numbered names (`numbered`) in `folder` that no file has, nor any of
/// the downloads `taken` goes to.
fn free_path(folder: &Path, name: &str, taken: &[PathBuf], _may: Blocking<'_>) -> PathBuf {
    (0..u32::MAX)
        .map(|n| folder.join(numbered(name, n)))
        .find(|path| !taken.contains(path) && path.symlink_metadata().is_err())
        .unwrap_or_else(|| folder.join(name))
}

/// A size as the list says it, in the Finder's units: 1 KB is 1000 bytes.
pub(super) fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1000.;
    let mut unit = 0;
    while value >= 999.95 && unit + 1 < UNITS.len() {
        value /= 1000.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

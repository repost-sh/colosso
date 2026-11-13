//! The pinned pages kept for the next launch (`docs/arc-sidebar.md` 11, `docs/browser.md` 10):
//! each by the address it showed, its title, the address it's pinned at and the project whose
//! profile it keeps, in the order of their tiles, with the one shown. In a file of their own
//! beside the window's, so a window file that can't be read never takes them with it.
//!
//! It's read by hand rather than by a shape: a field this build doesn't know is carried through
//! to the next write, a field left out has its default, an entry it can't read as a page stays in
//! the file as it was, and whatever version wrote it, it's read. A file that isn't this at all is
//! set aside rather than written over.

use std::path::{Path, PathBuf};

use gpui_kit::{App, Context};
use serde_json::{Map, Value, json};

use super::SAVE_DELAY;
use crate::browser;
use crate::kept_file::{self, KeptFile};
use crate::model::{Pinned, Tab};
use crate::shell::Shell;

/// What the file is called, in the app's data folder.
pub(super) const FILE: &str = "pinned-tabs.json";
/// What a file this build couldn't read is set aside as, beside it.
const ASIDE: &str = "pinned-tabs.unreadable.json";
/// The version this build writes. A later one is kept as it was read.
const VERSION: u64 = 1;

/// A pinned page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeptPinned {
    /// The address it showed; empty for a start page.
    pub url: String,
    /// Its title, until its page says otherwise.
    pub title: String,
    /// The address it's pinned at, where it goes back once closed; the one it showed, in a file
    /// that doesn't say.
    pub pinned_url: String,
    /// The project whose profile its page keeps, by its main checkout.
    pub project: PathBuf,
    /// Whatever else its entry held, written back as it was.
    pub more: Map<String, Value>,
}

/// What the file holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeptPins {
    /// In the order of their tiles.
    pub tabs: Vec<KeptPinned>,
    /// The one shown, by its place among `tabs`.
    pub shown: Option<usize>,
    /// The entries this build couldn't read as a page, written back after the others.
    pub unread: Vec<Value>,
    /// Whatever else the file held, written back as it was.
    pub more: Map<String, Value>,
    /// The version that wrote it, when it's later than this build's.
    pub version: u64,
}

impl KeptPinned {
    /// `entry` as a pinned page: an object with an address. What's missing or of another type
    /// than this build writes has its default.
    fn of(entry: &Value) -> Option<KeptPinned> {
        let mut more = entry.as_object()?.clone();
        let Some(Value::String(url)) = more.remove("url") else {
            return None;
        };
        let mut text = |name: &str| match more.remove(name) {
            Some(Value::String(text)) => text,
            _ => String::new(),
        };
        let (title, project) = (text("title"), text("project"));
        let pinned_url = match more.remove("pinnedUrl") {
            Some(Value::String(pinned)) => pinned,
            _ => url.clone(),
        };
        Some(KeptPinned {
            url,
            title,
            pinned_url,
            project: project.into(),
            more,
        })
    }

    fn entry(&self) -> Value {
        let mut entry = self.more.clone();
        entry.insert("url".into(), json!(self.url));
        entry.insert("title".into(), json!(self.title));
        entry.insert("pinnedUrl".into(), json!(self.pinned_url));
        entry.insert("project".into(), json!(self.project));
        Value::Object(entry)
    }
}

impl KeptPins {
    /// What `json` holds; `None` when it isn't a file of pinned pages at all: not JSON, not an
    /// object, or its `tabs` no list.
    pub fn read(json: &[u8]) -> Option<KeptPins> {
        let Ok(Value::Object(mut more)) = serde_json::from_slice(json) else {
            return None;
        };
        let entries = match more.remove("tabs") {
            Some(Value::Array(entries)) => entries,
            Some(_) => return None,
            None => Vec::new(),
        };
        let shown = (more.remove("shown")).and_then(|shown| usize::try_from(shown.as_u64()?).ok());
        let version = (more.remove("version")).and_then(|version| version.as_u64());
        let mut pins = KeptPins {
            more,
            version: version.unwrap_or(VERSION),
            ..KeptPins::default()
        };
        for (place, entry) in entries.into_iter().enumerate() {
            match KeptPinned::of(&entry) {
                Some(tab) => {
                    if shown == Some(place) {
                        pins.shown = Some(pins.tabs.len());
                    }
                    pins.tabs.push(tab);
                }
                None => pins.unread.push(entry),
            }
        }
        Some(pins)
    }

    /// The file's JSON.
    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut file = self.more.clone();
        file.insert("version".into(), json!(self.version.max(VERSION)));
        let shown = self.shown.filter(|shown| *shown < self.tabs.len());
        if let Some(shown) = shown {
            file.insert("shown".into(), json!(shown));
        }
        let tabs = self.tabs.iter().map(KeptPinned::entry);
        let tabs: Vec<Value> = tabs.chain(self.unread.iter().cloned()).collect();
        file.insert("tabs".into(), Value::Array(tabs));
        kept_file::encode(&Value::Object(file))
    }
}

/// What the file at `path` holds; nothing when there's none. One this build can't read at all
/// is set aside beside it, so the next write doesn't lose what it held.
pub(super) fn read_file(path: &Path) -> KeptPins {
    let json = match std::fs::read(path) {
        Ok(json) => json,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("couldn't read {}: {error}", path.display());
            }
            return KeptPins::default();
        }
    };
    KeptPins::read(&json).unwrap_or_else(|| {
        let aside = path.with_file_name(ASIDE);
        match std::fs::rename(path, &aside) {
            Ok(()) => log::warn!(
                "{} isn't a file of pinned tabs: kept as {}",
                path.display(),
                aside.display()
            ),
            Err(error) => log::warn!("couldn't set {} aside: {error}", path.display()),
        }
        KeptPins::default()
    })
}

/// The pinned pages' file: where it is, and what it holds or will once the write due is done.
#[derive(Debug, Default)]
pub struct PinsFile {
    file: KeptFile,
    kept: KeptPins,
}

impl PinsFile {
    /// What the file at `path` holds, if the app keeps one.
    pub(super) fn at(path: Option<PathBuf>) -> Self {
        let kept = path.as_deref().map(read_file).unwrap_or_default();
        PinsFile {
            file: KeptFile::new(path),
            kept,
        }
    }
}

impl Shell {
    /// Brings the pinned pages back as the file has them, once, as the window opens
    /// (`restore_pages`): in the order of their tiles, each titled as it was, with its site's
    /// icon when one is kept, the one shown shown. No page is made until its tile is picked, or
    /// it's the one shown.
    pub(super) fn restore_pinned(&mut self, cx: &mut Context<Self>) {
        let (kept, workspace) = (&self.kept.pins.kept, &mut self.workspace);
        for (place, tab) in kept.tabs.iter().enumerate() {
            let pane = browser::page_brought_back(&tab.url, &tab.title, cx);
            let pinned = Pinned {
                tab: Tab::Single(pane),
                home: tab.pinned_url.clone(),
                project: tab.project.clone(),
                more: tab.more.clone(),
            };
            workspace.add_pinned(pinned, kept.shown == Some(place));
        }
    }

    /// Has the pinned pages written a moment from now, after any change to the shell, as the
    /// window is (`keep_window`).
    pub(super) fn keep_pinned(&mut self, cx: &mut Context<Self>) {
        if self.kept.pins.file.changed() {
            let shell = cx.weak_entity();
            kept_file::write_later(cx, SAVE_DELAY, move |cx| {
                shell.update(cx, |shell, cx| shell.pinned_due(cx)).ok()?
            });
        }
    }

    /// Where to write the pinned pages as they are now, and their JSON, if a write is due, which
    /// then no longer is, and they aren't what the file holds already.
    pub(super) fn pinned_due(&mut self, cx: &App) -> Option<(PathBuf, Vec<u8>)> {
        let path = self.kept.pins.file.take_due()?;
        let now = self.pinned_now(cx);
        if now == self.kept.pins.kept {
            return None;
        }
        self.kept.pins.kept = now;
        Some((path, self.kept.pins.kept.encode()?))
    }

    /// What's kept: each pinned page as its pane shows it, with what its entry held that this
    /// build doesn't read, and the entries it couldn't read at all, as they were.
    fn pinned_now(&self, cx: &App) -> KeptPins {
        let before = &self.kept.pins.kept;
        let tabs = (self.workspace.pinned().iter())
            .map(|pinned| {
                let page = pinned.tab.first();
                KeptPinned {
                    url: page.address(cx).unwrap_or_default(),
                    title: page.title.to_string(),
                    pinned_url: pinned.home.clone(),
                    project: pinned.project.clone(),
                    more: pinned.more.clone(),
                }
            })
            .collect();
        KeptPins {
            tabs,
            shown: self.workspace.pinned_shown(),
            unread: before.unread.clone(),
            more: before.more.clone(),
            version: before.version,
        }
    }
}

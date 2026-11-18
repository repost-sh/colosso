//! The settings file: read at startup, taken up again when it's edited outside the app
//! (`watch.rs`), and the JSON written there once changes settle.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui_kit::App;

use super::migration::migrate;
use super::{Settings, Store};
use crate::blocking::Blocking;

/// The file as a look at it found it: when it last changed, and the settings it holds.
pub(super) struct Looked {
    pub modified: Option<SystemTime>,
    pub read: Result<Settings, String>,
}

/// The file at `path`, last changed at `modified`, read.
pub(super) fn look(path: &Path, modified: Option<SystemTime>, may: Blocking<'_>) -> Looked {
    let read = read(path, may).map(|mut settings| {
        settings.reconcile_spaces();
        settings
    });
    Looked { modified, read }
}

/// Takes up an edit made to the file outside the app since it was last read or written,
/// unless a change made in the app is still to be written; an edit that doesn't read as
/// settings, as one half done may not, is noted and left until the next.
pub(super) fn take_up(looked: Looked, cx: &mut App) {
    let store = cx.global::<Store>();
    let Some(path) = store.path.clone().filter(|_| !store.file.borrow().unsaved) else {
        return;
    };
    if looked.modified == store.file.borrow().seen {
        return;
    }
    store.file.borrow_mut().seen = looked.modified;
    match looked.read {
        Ok(settings) if settings != store.settings => cx.global_mut::<Store>().settings = settings,
        Ok(_) => {}
        Err(error) => log::warn!("not taking up {}: {error}", path.display()),
    }
}

/// When the file at `path` last changed, if it's there.
pub(super) fn modified(path: &Path, _: Blocking<'_>) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

pub(super) fn read(path: &Path, _: Blocking<'_>) -> Result<Settings, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let mut value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    migrate(&mut value);
    let mut settings: Settings =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.list_every_server();
    Ok(settings)
}

pub(super) fn load(path: &Path, may: Blocking<'_>) -> Settings {
    match read(path, may) {
        Ok(settings) => settings,
        Err(_) if !path.exists() => Settings::default(),
        Err(error) => {
            log::warn!("ignoring unreadable {}: {error}", path.display());
            Settings::default()
        }
    }
}

/// What's still to be written as the app quits, and the save waiting is let go: nothing, once
/// the last change is saved.
pub(super) fn due_at_quit(cx: &mut App) -> Option<(PathBuf, Vec<u8>)> {
    let unsaved = {
        let mut file = cx.global::<Store>().file.borrow_mut();
        file.pending = None;
        file.unsaved
    };
    if unsaved { encode(cx) } else { None }
}

/// The settings file's path and the JSON to write there.
pub(super) fn encode(cx: &mut App) -> Option<(PathBuf, Vec<u8>)> {
    let store = cx.global::<Store>();
    let path = store.path.clone()?;
    match serde_json::to_vec_pretty(&store.settings) {
        Ok(json) => Some((path, json)),
        Err(error) => {
            log::error!("couldn't encode the settings: {error}");
            None
        }
    }
}

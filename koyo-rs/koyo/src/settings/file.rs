//! The settings file: read at startup, taken up again when it's edited outside the app, and
//! the JSON written there once changes settle.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui_kit::App;

use super::migration::migrate;
use super::{Settings, Store};

/// Takes up an edit made to the file outside the app since it was last read or written,
/// unless a change made in the app is still to be written; an edit that doesn't read as
/// settings, as one half done may not, is noted and left until the next.
pub(super) fn reload(cx: &mut App) {
    let store = cx.global::<Store>();
    let Some(path) = store.path.clone().filter(|_| !store.unsaved) else {
        return;
    };
    let changed = modified(&path);
    if changed == store.seen {
        return;
    }
    let read = read(&path).map(|mut settings| {
        settings.reconcile_spaces();
        settings
    });
    let store = cx.global_mut::<Store>();
    store.seen = changed;
    match read {
        Ok(settings) if settings != store.settings => store.settings = settings,
        Ok(_) => {}
        Err(error) => log::warn!("not taking up {}: {error}", path.display()),
    }
}

/// When the file at `path` last changed, if it's there.
pub(super) fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

pub(super) fn read(path: &Path) -> Result<Settings, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    migrate(&mut value);
    let mut settings: Settings =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.list_every_server();
    Ok(settings)
}

pub(super) fn load(path: &Path) -> Settings {
    match read(path) {
        Ok(settings) => settings,
        // Before anything was saved.
        Err(_) if !path.exists() => Settings::default(),
        Err(error) => {
            log::warn!("ignoring unreadable {}: {error}", path.display());
            Settings::default()
        }
    }
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

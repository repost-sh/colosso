//! The settings file: read at startup, taken up again when it's edited outside the app, and
//! the JSON written there once changes settle.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::App;

use super::migration::migrate;
use super::{Settings, Store};
use crate::beat::is_frontmost;

/// How often the file is looked at while Koyo is in front, for an edit made to it outside the
/// app meanwhile, in an editor beside it say. One made while another app was in front is taken
/// up as Koyo comes back (`super::came_to_front`), so nothing looks while it's behind.
const LOOK_EVERY: Duration = Duration::from_secs(2);

/// Looks at the file every [`LOOK_EVERY`] until Koyo is found behind, in place of any looking
/// going on; with no file to look at, never.
pub(super) fn look_while_in_front(cx: &mut App) {
    if cx.global::<Store>().path.is_none() {
        return;
    }
    let looking = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(LOOK_EVERY).await;
            let in_front = cx.update(|cx| {
                let in_front = is_frontmost(cx);
                if in_front {
                    reload(cx);
                }
                in_front
            });
            if !in_front {
                return;
            }
        }
    });
    cx.global::<Store>().file.borrow_mut().looking = Some(looking);
}

/// Takes up an edit made to the file outside the app since it was last read or written,
/// unless a change made in the app is still to be written; an edit that doesn't read as
/// settings, as one half done may not, is noted and left until the next.
pub(super) fn reload(cx: &mut App) {
    let store = cx.global::<Store>();
    let Some(path) = store.path.clone().filter(|_| !store.file.borrow().unsaved) else {
        return;
    };
    let changed = modified(&path);
    if changed == store.file.borrow().seen {
        return;
    }
    let read = read(&path).map(|mut settings| {
        settings.reconcile_spaces();
        settings
    });
    store.file.borrow_mut().seen = changed;
    match read {
        Ok(settings) if settings != store.settings => cx.global_mut::<Store>().settings = settings,
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

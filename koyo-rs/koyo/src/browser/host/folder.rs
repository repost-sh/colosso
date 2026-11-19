//! The profiles' folder while no engine holds it. Work on it runs off the main thread, and the
//! engine's start waits until that work is done with the folder (`starting.rs`): a clear
//! moves the folder aside, so the start finds none, and deletes it after; and the folders of
//! profiles cleared or deleted before go as the app launches.

use std::future::Future;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::channel::oneshot;
use gpui_kit::{App, Global};

use crate::blocking::{self, Blocking};

/// Work on the profiles' folder under way, which the engine's start waits for: each says once
/// it's done with the folder.
#[derive(Default)]
struct Readying(Vec<oneshot::Receiver<()>>);

impl Global for Readying {}

/// Runs `work` on the profiles' folder off the main thread, handing it what it drops once it's
/// done with the folder: the engine's start waits until then.
fn ready(
    cx: &mut App,
    work: impl FnOnce(&Path, oneshot::Sender<()>, Blocking<'_>) + Send + 'static,
) {
    let Some(root) = super::profiles(cx) else {
        return;
    };
    let (done, waited) = oneshot::channel();
    blocking::spawn(cx.background_executor(), move |may| work(&root, done, may)).detach();
    cx.default_global::<Readying>().0.push(waited);
}

/// Ready once the work on the profiles' folder asked for so far is done with it.
pub(super) fn readied(cx: &mut App) -> impl Future<Output = ()> + Send + 'static {
    let waits = std::mem::take(&mut cx.default_global::<Readying>().0);
    async move {
        futures::future::join_all(waits).await;
    }
}

/// Has the folders of profiles cleared or deleted before go, as the app launches, rather than
/// as the engine next starts, which may be launches away.
pub(in crate::browser) fn drop_cleared(cx: &mut App) {
    ready(cx, |root, done, _| {
        koyo_browser_host::drop_cleared(root);
        drop(done);
    });
}

/// Clears the profiles' folder, which no engine holds: it's moved aside, so an engine starting
/// finds none, then deleted, with any an earlier clear moved aside and left.
pub(super) fn clear(cx: &mut App) {
    ready(cx, clear_unheld);
}

fn clear_unheld(root: &Path, moved: oneshot::Sender<()>, _: Blocking<'_>) {
    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else {
        return;
    };
    let aside = format!("{}.cleared-", name.to_string_lossy());
    let at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let moving = std::fs::rename(root, parent.join(format!("{aside}{}", at.as_nanos())));
    if let Err(error) = moving.or_else(gone) {
        log::warn!("couldn't clear {}: {error}", root.display());
    }
    drop(moved);
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&aside) {
            continue;
        }
        if let Err(error) = std::fs::remove_dir_all(entry.path()).or_else(gone) {
            log::warn!("couldn't delete {}: {error}", entry.path().display());
        }
    }
}

/// `error`, unless what it's about was gone already, which is what was asked.
fn gone(error: io::Error) -> io::Result<()> {
    match error.kind() {
        io::ErrorKind::NotFound => Ok(()),
        _ => Err(error),
    }
}

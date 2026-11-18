//! The icons of the hosts pages were visited on, so the launcher shows a site's own icon beside
//! it, as tabs do: kept as the browser fetched them, a PNG a host, in a folder beside the history.
//! Asking for one never reads the disk. The first time a host is asked for, its file is read on
//! the background executor, and what observes the icons hears once it lands
//! ([`observe_icons`]): a launcher row shows a globe for a frame, then the site's icon. The
//! icons of the pages a window brings back are read with the window's file ([`read_icons`]), so
//! its tabs come back with theirs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{App, Context, Global, Image, ImageFormat, Subscription};

use super::address::host;
use crate::blocking::{self, Blocking};
use crate::settings;

/// The folder, in the app's data folder.
const FOLDER: &str = "history-icons";

/// What's known of a host's icon.
enum Kept {
    /// Its file is being read.
    Loading,
    /// It has none.
    Missing,
    Icon(Arc<Image>),
}

/// The icons known, by host, and the folder they're kept in, if the app keeps them. Asking
/// marks a host loading in the cell, so it tells no observer; an icon landing does.
#[derive(Default)]
struct Icons {
    folder: Option<PathBuf>,
    by_host: RefCell<HashMap<String, Kept>>,
}

impl Global for Icons {}

/// The icons kept in the app's data folder; tests keep none.
pub(super) fn init(cx: &mut App) {
    let folder = settings::data_dir()
        .filter(|_| !cfg!(test))
        .map(|data| data.join(FOLDER));
    cx.set_global(Icons {
        folder,
        by_host: RefCell::default(),
    });
}

/// From now on the icons are kept in `folder`, as if the app had just started.
#[cfg(test)]
pub fn keep_icons_in(folder: &Path, cx: &mut App) {
    cx.set_global(Icons {
        folder: Some(folder.to_owned()),
        by_host: RefCell::default(),
    });
}

/// Where the icon of `host` is kept in `folder`: its name, made safe for a file's.
fn file(folder: &Path, host: &str) -> PathBuf {
    let name: String = (host.chars())
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    folder.join(format!("{name}.png"))
}

fn kept(png: Option<Vec<u8>>) -> Kept {
    png.map_or(Kept::Missing, |png| {
        Kept::Icon(Arc::new(Image::from_bytes(ImageFormat::Png, png)))
    })
}

/// Keeps `icon`, the PNG a page at `url` named, as its host's. One kept already as it is
/// changes nothing, and tells no observer.
pub fn keep_icon(url: &str, icon: &Arc<Image>, cx: &mut App) {
    let Some(host) = host(url).filter(|_| icon.format == ImageFormat::Png) else {
        return;
    };
    let known = cx.try_global::<Icons>().is_some_and(|icons| {
        let by_host = icons.by_host.borrow();
        matches!(by_host.get(&host), Some(Kept::Icon(known)) if known.bytes == icon.bytes)
    });
    if known {
        return;
    }
    let icons = cx.default_global::<Icons>();
    icons.by_host.get_mut().insert(host.clone(), Kept::Icon(Arc::clone(icon)));
    let (Some(folder), bytes) = (icons.folder.clone(), icon.bytes.clone()) else {
        return;
    };
    blocking::spawn(cx.background_executor(), move |_| {
        let path = file(&folder, &host);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, &bytes));
        if let Err(error) = written {
            log::warn!("couldn't keep the icon at {}: {error}", path.display());
        }
    })
    .detach();
}

/// The icon of the host of the page at `url`, if it has one kept and it's been read. The first
/// time a host is asked for, its file is read off the main thread, and what observes the icons
/// hears once it lands.
pub fn icon(url: &str, cx: &App) -> Option<Arc<Image>> {
    let host = host(url)?;
    let icons = cx.try_global::<Icons>()?;
    let mut by_host = icons.by_host.borrow_mut();
    match by_host.get(&host) {
        Some(Kept::Icon(icon)) => return Some(Arc::clone(icon)),
        Some(Kept::Loading | Kept::Missing) => return None,
        None => {}
    }
    let Some(folder) = icons.folder.clone() else {
        by_host.insert(host, Kept::Missing);
        return None;
    };
    by_host.insert(host.clone(), Kept::Loading);
    let path = file(&folder, &host);
    let read = blocking::spawn(cx.background_executor(), move |_| std::fs::read(&path).ok());
    cx.spawn(async move |cx| {
        let landed = kept(read.await);
        if matches!(landed, Kept::Icon(_)) {
            cx.update_global::<Icons, _>(|icons, _| icons.by_host.get_mut().insert(host, landed));
        } else {
            cx.update(|cx| {
                let icons = cx.try_global::<Icons>();
                icons.map(|icons| icons.by_host.borrow_mut().insert(host, landed));
            });
        }
    })
    .detach();
    None
}

/// Calls `landed` whenever an icon lands, or one is kept or forgotten.
pub fn observe_icons<V: 'static>(
    cx: &mut Context<V>,
    landed: impl FnMut(&mut V, &mut Context<V>) + 'static,
) -> Subscription {
    cx.observe_global::<Icons>(landed)
}

/// Reads the icons of the hosts of the pages at `addresses` before anything asks for them, as
/// those of a window's pages are with its file at launch.
pub fn read_icons(addresses: &[&str], _: Blocking<'_>, cx: &mut App) {
    let icons = cx.default_global::<Icons>();
    let Some(folder) = icons.folder.clone() else {
        return;
    };
    let by_host = icons.by_host.get_mut();
    for host in addresses.iter().filter_map(|url| host(url)) {
        let path = file(&folder, &host);
        if let Entry::Vacant(slot) = by_host.entry(host) {
            slot.insert(kept(std::fs::read(&path).ok()));
        }
    }
}

/// Forgets every icon, as clearing the browser's data does.
pub(super) fn clear(cx: &mut App) {
    let icons = cx.default_global::<Icons>();
    icons.by_host.get_mut().clear();
    if let Some(folder) = icons.folder.clone() {
        blocking::spawn(cx.background_executor(), move |_| {
            if let Err(error) = std::fs::remove_dir_all(&folder)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                log::warn!("couldn't clear the icons at {}: {error}", folder.display());
            }
        })
        .detach();
    }
}

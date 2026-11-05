//! The icons of the hosts pages were visited on, so the launcher shows a site's own icon beside
//! it, as tabs do: kept as the browser fetched them, a PNG a host, in a folder beside the history,
//! and read from there the first time one is asked for.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{App, Global, Image, ImageFormat};

use super::rank::host;
use crate::settings;

/// The folder, in the app's data folder.
const FOLDER: &str = "history-icons";

/// The icons known, by host: `None` for a host looked for that has none.
#[derive(Default)]
struct Icons {
    by_host: HashMap<String, Option<Arc<Image>>>,
}

impl Global for Icons {}

fn folder() -> Option<PathBuf> {
    settings::data_dir().filter(|_| !cfg!(test)).map(|data| data.join(FOLDER))
}

/// Where the icon of `host` is kept: its name, made safe for a file's.
fn file(host: &str) -> Option<PathBuf> {
    let name: String = (host.chars())
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    folder().map(|folder| folder.join(format!("{name}.png")))
}

/// Keeps `icon`, the PNG a page at `url` named, as its host's.
pub fn keep_icon(url: &str, icon: &Arc<Image>, cx: &mut App) {
    let Some(host) = host(url) else {
        return;
    };
    let icons = cx.default_global::<Icons>();
    let known = icons.by_host.get(&host).and_then(Option::as_ref);
    if known.is_some_and(|known| known.bytes == icon.bytes) || icon.format != ImageFormat::Png {
        return;
    }
    icons.by_host.insert(host.clone(), Some(Arc::clone(icon)));
    let (Some(path), bytes) = (file(&host), icon.bytes.clone()) else {
        return;
    };
    cx.background_executor()
        .spawn(async move {
            let written = (path.parent().map_or(Ok(()), std::fs::create_dir_all))
                .and_then(|()| std::fs::write(&path, bytes));
            if let Err(error) = written {
                log::warn!("couldn't keep the icon at {}: {error}", path.display());
            }
        })
        .detach();
}

/// The icon of the host of the page at `url`, if it has one kept.
pub fn icon(url: &str, cx: &mut App) -> Option<Arc<Image>> {
    let host = host(url)?;
    let icons = cx.default_global::<Icons>();
    if let Some(known) = icons.by_host.get(&host) {
        return known.clone();
    }
    let read = file(&host).and_then(|path| std::fs::read(path).ok());
    let icon = read.map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png)));
    icons.by_host.insert(host, icon.clone());
    icon
}

/// Forgets every icon, as clearing the browser's data does.
pub(super) fn clear(cx: &mut App) {
    cx.default_global::<Icons>().by_host.clear();
    if let Some(folder) = folder() {
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = std::fs::remove_dir_all(&folder)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    log::warn!("couldn't clear the icons at {}: {error}", folder.display());
                }
            })
            .detach();
    }
}

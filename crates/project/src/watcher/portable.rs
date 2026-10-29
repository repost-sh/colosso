//! The platform's watcher through `notify`, everywhere but macOS: inotify on Linux,
//! `ReadDirectoryChangesW` on Windows. Folders are added and taken away as checkouts come
//! and go, so nothing is watched twice.

use std::path::PathBuf;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};

use super::{Batch, report};
use crate::places::text;

pub(crate) struct Platform {
    watcher: RecommendedWatcher,
    folders: Vec<PathBuf>,
}

impl Platform {
    pub(crate) fn start(folders: Vec<PathBuf>) -> Result<Platform, String> {
        let watcher = notify::recommended_watcher(take).map_err(|error| error.to_string())?;
        let mut platform = Platform {
            watcher,
            folders: Vec::new(),
        };
        platform.cover(folders)?;
        Ok(platform)
    }

    pub(crate) fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    /// Watches `folders` from now on: those not watched yet are added, and those no longer
    /// wanted taken away.
    pub(crate) fn cover(&mut self, folders: Vec<PathBuf>) -> Result<(), String> {
        for folder in folders
            .iter()
            .filter(|folder| !self.folders.contains(folder))
        {
            (self.watcher.watch(folder, RecursiveMode::Recursive))
                .map_err(|error| format!("watching {}: {error}", folder.display()))?;
        }
        for folder in self
            .folders
            .iter()
            .filter(|folder| !folders.contains(folder))
        {
            if let Err(error) = self.watcher.unwatch(folder) {
                log::debug!("no longer watching {}: {error}", folder.display());
            }
        }
        self.folders = folders;
        Ok(())
    }
}

/// Takes one report from `notify`, on its thread, copying it off to be sorted.
fn take(event: notify::Result<Event>) {
    let mut batch = Batch::default();
    match event {
        Ok(event) => {
            let lost = event.need_rescan();
            for path in &event.paths {
                batch.push(&text(path), lost);
            }
            // Too much happened for the platform to say what, or where.
            if lost && event.paths.is_empty() {
                batch.push("", true);
            }
        }
        Err(error) => {
            log::debug!("the watcher lost track: {error}");
            batch.push("", true);
        }
    }
    report(batch);
}

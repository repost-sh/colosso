//! Writing a file the app keeps.

use std::path::Path;

/// Writes through a temporary file, so a crash mid-write never leaves half a file behind.
pub fn write(path: &Path, json: &[u8]) {
    let temporary = path.with_extension("json.tmp");
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temporary, json))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        log::error!("couldn't save {}: {error}", path.display());
    }
}

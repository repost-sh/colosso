//! The fallback socket path, for a data folder too deep for `sun_path`. The temporary folder
//! may be shared by every user (`/tmp`), so an existing fallback folder is used only when it
//! is a real folder of this user's that admits nobody else; the listener creates it 0700
//! when it's missing.

use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::{FALLBACK_SOCKET_FILE, MAX_SOCKET_PATH, fallback_dir, user_id};
use crate::ControlError;

/// The socket path in the fallback folder of `dir`, in `temp`.
pub(super) fn socket_path(dir: &Path, temp: &Path) -> Result<PathBuf, ControlError> {
    let folder = fallback_dir(dir, temp);
    let path = folder.join(FALLBACK_SOCKET_FILE);
    let length = path.as_os_str().len();
    if length > MAX_SOCKET_PATH {
        return Err(ControlError::invalid_input(format!(
            "The control socket of {} would be {} ({length} bytes), longer than the {MAX_SOCKET_PATH} a socket path may have. Use a shorter KOYO_DATA_DIR or TMPDIR.",
            dir.display(),
            path.display()
        )));
    }
    check(&folder)?;
    Ok(path)
}

/// Refuses an existing fallback `folder` that isn't a private folder of this user's. A
/// missing one is fine: binding creates it.
fn check(folder: &Path) -> Result<(), ControlError> {
    let found = match fs::symlink_metadata(folder) {
        Ok(found) => found,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(refused(folder, &format!("can't be read ({error})"))),
    };
    if !found.file_type().is_dir() {
        return Err(refused(folder, "isn't a folder"));
    }
    if found.uid() != user_id() {
        return Err(refused(folder, "belongs to another user"));
    }
    let mode = found.permissions().mode() & 0o777;
    if mode != 0o700 {
        return Err(refused(
            folder,
            &format!("has mode {mode:o}, not 700, so other users may reach it"),
        ));
    }
    Ok(())
}

fn refused(folder: &Path, why: &str) -> ControlError {
    ControlError::invalid_input(format!(
        "The control socket folder {} {why}. Remove it, or use a shorter KOYO_DATA_DIR.",
        folder.display()
    ))
}

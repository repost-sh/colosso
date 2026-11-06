//! Where the app listens (`docs/app-control.md` 6): a socket file in the data folder's own
//! `control/` subfolder, `<data_dir>/control/koyo-control.sock`. The data folder is the
//! app's (`KOYO_DATA_DIR` moves it), so the app, its relays and the CLI agree on it. A
//! data folder too deep for a socket path gets a private folder named after it under the
//! temporary folder instead: `$TMPDIR/koyo-<uid>-<hash8>/control.sock`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::ControlError;

/// The app's folder name under the platform's data folder, as `settings::data_dir` and the
/// terminals' `paths::APP_ID` name it.
pub const APP_ID: &str = "dev.koyo.desktop";
/// The subfolder of the data folder that holds the socket. Only this folder is made private
/// (0700); the data folder keeps its permissions.
pub const SOCKET_DIR: &str = "control";
/// The socket file's name in [`SOCKET_DIR`].
pub const SOCKET_FILE: &str = "koyo-control.sock";
/// The socket file's name in the fallback folder ([`fallback_dir`]).
pub const FALLBACK_SOCKET_FILE: &str = "control.sock";
/// The longest socket path used, in bytes. macOS allows 103 in `sun_path`; the margin keeps
/// a path that fits there fitting everywhere.
pub const MAX_SOCKET_PATH: usize = 100;

/// The app's data folder: `KOYO_DATA_DIR` when set, else `dev.koyo.desktop` in the
/// platform's data folder (Application Support on macOS). The same folder as the app's
/// `settings::data_dir`.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("KOYO_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(APP_ID)))
}

/// The control socket of one data folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    path: PathBuf,
}

impl Endpoint {
    /// The socket of the data folder this process sees ([`data_dir`]).
    ///
    /// # Errors
    /// `internal` when the platform has no data folder; as [`Endpoint::for_data_dir`].
    pub fn from_env() -> Result<Endpoint, ControlError> {
        let dir = data_dir().ok_or_else(|| {
            ControlError::internal("This system has no data folder for Koyo. Set KOYO_DATA_DIR.")
        })?;
        Endpoint::for_data_dir(&dir)
    }

    /// The socket of the data folder `dir`, with the fallback folder in the temporary folder
    /// (`TMPDIR`, else `/tmp`).
    ///
    /// # Errors
    /// As [`Endpoint::for_data_dir_in`].
    pub fn for_data_dir(dir: &Path) -> Result<Endpoint, ControlError> {
        Endpoint::for_data_dir_in(dir, &std::env::temp_dir())
    }

    /// The socket of the data folder `dir`: `<dir>/control/koyo-control.sock`, or, when
    /// that path is longer than [`MAX_SOCKET_PATH`] bytes, `control.sock` in the private
    /// folder [`fallback_dir`] makes in `temp`.
    ///
    /// # Errors
    /// `invalid_input` when the fallback path is too long as well, or the fallback folder
    /// exists but isn't a private folder of this user's.
    pub fn for_data_dir_in(dir: &Path, temp: &Path) -> Result<Endpoint, ControlError> {
        let path = dir.join(SOCKET_DIR).join(SOCKET_FILE);
        if path.as_os_str().len() <= MAX_SOCKET_PATH {
            return Ok(Endpoint { path });
        }
        #[cfg(unix)]
        {
            fallback::socket_path(dir, temp).map(|path| Endpoint { path })
        }
        #[cfg(not(unix))]
        {
            let _ = temp;
            Err(ControlError::invalid_input(
                "The control socket needs a Unix system.",
            ))
        }
    }

    /// An endpoint at a socket path of the caller's choosing, for tests and tools.
    pub fn at(path: impl Into<PathBuf>) -> Endpoint {
        Endpoint { path: path.into() }
    }

    /// The socket file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The folder in `temp` that holds the socket of the data folder `dir` when
/// `<dir>/control/` is too deep: `koyo-<uid>-<hash8>`, the user's id and 8 hex digits of
/// a SHA-256 of the folder's path (trailing slashes left out). The listener makes it 0700.
pub fn fallback_dir(dir: &Path, temp: &Path) -> PathBuf {
    let path = dir.as_os_str().to_string_lossy();
    let trimmed = path.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
    let digest = Sha256::digest(trimmed.as_bytes());
    let mut name = format!("koyo-{}-", user_id());
    for byte in &digest[..4] {
        let _ = write!(name, "{byte:02x}");
    }
    temp.join(name)
}

#[cfg(unix)]
fn user_id() -> u32 {
    // SAFETY: `getuid` takes nothing, borrows nothing and always succeeds.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn user_id() -> u32 {
    0
}

#[cfg(unix)]
mod fallback;

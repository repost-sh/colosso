//! The control socket on Unix: the app's [`Listener`], and the line-framed [`Connection`]s
//! both sides talk over. The socket's folder is made 0700 and the socket 0600, so only this
//! user reaches it; the data folder around it keeps its permissions.

use std::fmt;
use std::fs::{self, DirBuilder, Permissions};
use std::io::{self, BufReader};
use std::net::Shutdown;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Endpoint;
use crate::frame::{self, FrameError};

/// Why the app couldn't listen.
#[derive(Debug)]
pub enum BindError {
    /// Another Koyo already answers on this socket; it's left alone.
    InUse(PathBuf),
    /// Anything else.
    Io(io::Error),
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindError::InUse(path) => write!(
                f,
                "Another Koyo already answers on {}; this one runs without app control.",
                path.display()
            ),
            BindError::Io(error) => write!(f, "Can't listen for app control: {error}"),
        }
    }
}

impl std::error::Error for BindError {}

impl From<io::Error> for BindError {
    fn from(error: io::Error) -> BindError {
        BindError::Io(error)
    }
}

/// The app's end of the socket. Dropping it removes the socket file, if the file is still
/// this listener's.
pub struct Listener {
    socket: UnixListener,
    path: PathBuf,
    /// The socket file's device and inode, so dropping never removes a socket another app
    /// bound after this one.
    identity: (u64, u64),
}

impl Listener {
    /// Listens on `endpoint`, making its folder (0700) and the socket (0600). A socket file
    /// nothing answers on, left by an app that crashed, is replaced; a live one is refused.
    ///
    /// # Errors
    /// [`BindError::InUse`] when another app answers there; [`BindError::Io`] otherwise.
    pub fn bind(endpoint: &Endpoint) -> Result<Listener, BindError> {
        let path = endpoint.path().to_path_buf();
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            private_folder(dir)?;
        }
        match fs::symlink_metadata(&path) {
            Ok(found) if !found.file_type().is_socket() => {
                return Err(BindError::Io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists and isn't a socket", path.display()),
                )));
            }
            Ok(_) => {
                if UnixStream::connect(&path).is_ok() {
                    return Err(BindError::InUse(path));
                }
                fs::remove_file(&path)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let socket = UnixListener::bind(&path)?;
        fs::set_permissions(&path, Permissions::from_mode(0o600))?;
        let found = fs::symlink_metadata(&path)?;
        Ok(Listener {
            socket,
            path,
            identity: (found.dev(), found.ino()),
        })
    }

    /// Waits for the next connection.
    ///
    /// # Errors
    /// When accepting fails.
    pub fn accept(&self) -> io::Result<Connection> {
        self.socket
            .accept()
            .map(|(stream, _)| Connection { stream })
    }

    /// The socket file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|found| (found.dev(), found.ino()) == self.identity)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Makes `dir` a folder only its owner may enter. The data folder above it is created with
/// the usual permissions when missing, and never changed.
fn private_folder(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    if !fs::symlink_metadata(dir)?.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists and isn't a folder", dir.display()),
        ));
    }
    fs::set_permissions(dir, Permissions::from_mode(0o700))
}

/// Whether a connect error means nothing listens there, so the app isn't running.
pub fn nobody_listening(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

/// One connection, from either side, before it's split into its reading and writing ends.
pub struct Connection {
    stream: UnixStream,
}

impl Connection {
    /// Connects to the app on `endpoint`.
    ///
    /// # Errors
    /// `NotFound` or `ConnectionRefused` when nothing listens there ([`nobody_listening`]).
    pub fn connect(endpoint: &Endpoint) -> io::Result<Connection> {
        UnixStream::connect(endpoint.path()).map(|stream| Connection { stream })
    }

    /// A connected pair, for tests that need no socket file.
    ///
    /// # Errors
    /// When the system can't make one.
    pub fn pair() -> io::Result<(Connection, Connection)> {
        let (a, b) = UnixStream::pair()?;
        Ok((Connection { stream: a }, Connection { stream: b }))
    }

    /// Splits the connection into a reader for one thread and a writer any thread may use.
    ///
    /// # Errors
    /// When the socket can't be duplicated.
    pub fn split(self) -> io::Result<(FrameReader, FrameWriter)> {
        let reading = self.stream.try_clone()?;
        let control = self.stream.try_clone()?;
        Ok((
            FrameReader {
                reader: BufReader::new(reading),
            },
            FrameWriter {
                stream: Arc::new(Mutex::new(self.stream)),
                control: Arc::new(control),
            },
        ))
    }
}

/// A connection's reading end: one frame per line.
pub struct FrameReader {
    reader: BufReader<UnixStream>,
}

impl FrameReader {
    /// The next line of at most `limit` bytes, without its newline; `None` when the peer
    /// hung up.
    ///
    /// # Errors
    /// As [`frame::read_line`].
    pub fn read_line(&mut self, limit: usize) -> Result<Option<Vec<u8>>, FrameError> {
        frame::read_line(&mut self.reader, limit)
    }

    /// The next frame; `None` when the peer hung up.
    ///
    /// # Errors
    /// As [`frame::read_frame`].
    pub fn read<T: DeserializeOwned>(&mut self, limit: usize) -> Result<Option<T>, FrameError> {
        frame::read_frame(&mut self.reader, limit)
    }

    /// Limits how long a read waits; `None` waits for as long as it takes.
    ///
    /// # Errors
    /// When the system refuses the timeout.
    pub fn set_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.reader.get_ref().set_read_timeout(timeout)
    }
}

/// A connection's writing end, shared by every thread that answers or calls on it; each
/// frame goes out whole.
#[derive(Clone)]
pub struct FrameWriter {
    stream: Arc<Mutex<UnixStream>>,
    /// The same socket outside the lock, so a shutdown never waits for a blocked write.
    control: Arc<UnixStream>,
}

impl FrameWriter {
    /// Writes one frame of at most `limit` bytes and its newline.
    ///
    /// # Errors
    /// As [`frame::write_frame`].
    pub fn write<T: Serialize>(&self, frame: &T, limit: usize) -> Result<(), FrameError> {
        let mut stream = self.stream.lock();
        frame::write_frame(&mut *stream, frame, limit)
    }

    /// Limits how long a write waits for a peer that doesn't read.
    ///
    /// # Errors
    /// When the system refuses the timeout.
    pub fn set_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.control.set_write_timeout(timeout)
    }

    /// Ends the connection both ways, waking whoever reads it. Writing after it fails.
    pub fn shutdown(&self) {
        let _ = self.control.shutdown(Shutdown::Both);
    }
}

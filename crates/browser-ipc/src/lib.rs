//! The protocol between `Colosso` and its browser host.
//!
//! The app starts the host from the installed engine and talks to it over a
//! socket pair whose host end the host inherits ([`inherit`]). Every message is one frame: a 4-byte
//! little-endian length, then that many bytes of JSON. The app speaks first
//! with [`Hello`], carrying the protocol version and a one-time token the
//! host was started with; the host answers [`HostEvent::Ready`] or closes.
//!
//! Pixels never travel over the socket. Each pane renders into a small ring
//! of shared surfaces the host hands over once (on macOS, `IOSurface` mach
//! ports through [`mach`]); per frame the host only sends which slot is
//! ready and what changed.
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

#[cfg(unix)]
pub mod client;
#[cfg(unix)]
pub mod inherit;
#[cfg(target_os = "macos")]
pub mod mach;

mod dialogs;
mod events;
mod input;
mod requests;

pub use dialogs::{Answer, Dialog, FileMode, MenuEntry, MenuTarget, Permission};
pub use events::{Cursor, HostEvent, PageState};
pub use input::{KeyKind, Modifiers, MouseButton, MouseKind, PixelSize, Point, Viewport};
pub use requests::{AppRequest, Edit, Go, Ime, ViewAction};

/// The protocol version. The app only starts engines whose `engine.json`
/// names this version; both sides refuse a [`Hello`] that differs.
///
/// 2: `DevTools` panes ([`AppRequest::OpenDevTools`], [`HostEvent::Closed`],
/// [`HostEvent::OpenUrl`]).
/// 3: agent control over the page's `DevTools` protocol session
/// ([`AppRequest::Cdp`], [`HostEvent::CdpReply`], [`HostEvent::CdpEvent`]).
pub const PROTOCOL: u32 = 5;

/// Frames larger than this are refused, so a confused peer cannot make the
/// other side allocate without bound.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Surfaces in each pane's ring: one shown, one being drawn, one spare.
pub const RING: u8 = 3;

/// The environment variable carrying the number of the inherited file
/// descriptor that is the host's end of the app's socket.
pub const ENV_SOCKET_FD: &str = "COLOSSO_BROWSER_SOCKET_FD";
/// The environment variable carrying the one-time token to the host.
pub const ENV_TOKEN: &str = "COLOSSO_BROWSER_TOKEN";
/// The environment variable naming the mach service the app checked in, to
/// which the host sends its frame surfaces (macOS).
pub const ENV_SURFACE_SERVICE: &str = "COLOSSO_BROWSER_SURFACES";
/// The environment variable with the folder for CEF's caches and profiles.
pub const ENV_PROFILE_ROOT: &str = "COLOSSO_BROWSER_PROFILES";

/// The first frame on a connection, from the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    /// The app's [`PROTOCOL`].
    pub protocol: u32,
    /// The token the host was started with.
    pub token: String,
}

/// A pane, by the app's id for it.
pub type PaneId = u64;

///
/// # Errors
/// When the message is larger than [`MAX_FRAME`] or the writer fails.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    // The length goes in front once the body is written after it.
    let mut frame = vec![0u8; 4];
    serde_json::to_writer(&mut frame, message).map_err(io::Error::other)?;
    let body = frame.len() - 4;
    if body > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "message too large",
        ));
    }
    let len = u32::try_from(body).map_err(io::Error::other)?;
    frame[..4].copy_from_slice(&len.to_le_bytes());
    writer.write_all(&frame)?;
    writer.flush()
}

/// Reads one frame. `Ok(None)` is a clean end of stream between frames.
/// Readers of a stream of frames should be buffered, so a frame is one read
/// rather than two.
///
/// # Errors
/// When the stream ends inside a frame, a frame is larger than
/// [`MAX_FRAME`], or its body is not the expected message.
pub fn read_frame<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> io::Result<Option<T>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Checks a [`Hello`] on the host side.
///
/// # Errors
/// A message for the app when the version or token differs.
pub fn check_hello(hello: &Hello, token: &str) -> Result<(), String> {
    if hello.protocol != PROTOCOL {
        return Err(format!(
            "The browser engine speaks protocol {PROTOCOL}, the app {}.",
            hello.protocol
        ));
    }
    // Compare without an early exit on the first differing byte.
    let same = hello.token.len() == token.len()
        && hello
            .token
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0;
    if same {
        Ok(())
    } else {
        Err("The browser host refused the connection.".into())
    }
}

#[cfg(test)]
mod tests;

//! The protocol between `Koyo` and its browser host.
//!
//! The app starts the host in its own process, from the engine in its bundle, and talks to
//! it over a socket pair. Every message is one frame: a 4-byte little-endian length, then
//! that many bytes of JSON. The app speaks first with [`Hello`], carrying the protocol
//! version and a one-time token the host was started with; the host answers
//! [`HostEvent::Ready`] or closes.
//!
//! A page's picture and its pointer and keys never travel here: the page's own views are in
//! the app's window, where Chromium draws them and `AppKit` hands them their input
//! (`docs/native-pages.md`). [`mach`] and [`inherit`] are how the simulator's and the agent
//! desktop's hosts, processes of their own, hand over their pictures and inherit a socket.
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

#[cfg(unix)]
pub mod client;
#[cfg(unix)]
pub mod inherit;
#[cfg(target_os = "macos")]
pub mod mach;
pub mod profiles;

mod devtools;
mod dialogs;
mod events;
mod extensions;
mod input;
mod passkeys;
mod requests;

pub use devtools::{DevToolsAsk, DevToolsDock};
pub use dialogs::{Answer, Dialog, FileMode, MenuEntry, MenuTarget, Permission};
pub use events::{Cursor, HostEvent, PageState, SiteSetting};
pub use extensions::{Corner, Extension, ExtensionAction, ExtensionChange, ExtensionSource};
pub use input::{Point, Viewport};
pub use passkeys::{
    Assertion, AuthenticatorSelection, Bytes, Created, Creation, CredentialDescriptor,
    CredentialParameters, Extensions, Got, PasskeyAnswer, PasskeyAsk, PasskeyError, Refusal,
    RelyingParty, User,
};
pub use requests::{AppRequest, Edit, Go, ViewAction};

/// The protocol version. The app only starts engines whose `engine.json`
/// names this version; both sides refuse a [`Hello`] that differs.
///
/// 2: `DevTools` panes ([`AppRequest::OpenDevTools`], [`HostEvent::Closed`],
/// [`HostEvent::OpenUrl`]).
/// 3: agent control over the page's `DevTools` protocol session
/// ([`AppRequest::Cdp`], [`HostEvent::CdpReply`], [`HostEvent::CdpEvent`]).
/// 6: page icons ([`PageState::favicon`], [`HostEvent::Favicon`]).
/// 8: a page's full screen ([`HostEvent::Fullscreen`], [`AppRequest::ExitFullscreen`]).
/// 9: passkeys ([`Hello::passkeys`], [`HostEvent::Passkey`], [`HostEvent::PasskeyWithdrawn`],
/// [`AppRequest::Passkey`]).
/// 10: a page's keys, which `AppKit` hands it, count as the person's ([`HostEvent::Typed`]), and browsing data is
/// cleared without stopping the host ([`AppRequest::ClearData`]).
/// 11: what a page's site is allowed and blocked from ([`AppRequest::SiteSettings`],
/// [`AppRequest::SetSiteSetting`], [`HostEvent::SiteSettings`]).
/// 12: `DevTools` with their own controls: the side they open at
/// ([`AppRequest::OpenDevTools`]'s `dock`), and what their dock menu, × and device toolbar
/// toggle ask ([`HostEvent::DevTools`]).
/// 13: pages are native and nothing else: no frames or surfaces from the host, no pointer,
/// wheel, pinch, keys or composition from the app, and `Resize` only sizes a page no pane
/// draws (`docs/native-pages.md`).
/// 14: no `Focus`: the app's keyboard moves a page's first responder itself, in its own
/// process (`docs/keyboard.md`).
/// 15: browser extensions (`docs/extensions.md`): a profile's extensions
/// ([`HostEvent::Extensions`]) and their toolbar actions ([`HostEvent::ExtensionAction`]), an
/// action run ([`AppRequest::ExtensionAct`]), an extension turned off, pinned or removed
/// ([`AppRequest::ChangeExtension`]), the page the person is on ([`AppRequest::Active`]), and
/// the tabs extensions open, bring forward and close ([`HostEvent::TabOpened`],
/// [`HostEvent::TabActivated`], [`HostEvent::TabClosed`]).
pub const PROTOCOL: u32 = 15;

/// Frames larger than this are refused, so a confused peer cannot make the
/// other side allocate without bound.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Surfaces in a ring sent over a mach service ([`mach`]): one shown, one being drawn, one
/// spare. The device and desktop hosts send their pictures so.
pub const RING: u8 = 3;

/// The environment variable carrying the number of the inherited file
/// descriptor that is the host's end of the app's socket.
pub const ENV_SOCKET_FD: &str = "KOYO_BROWSER_SOCKET_FD";
/// The environment variable carrying the one-time token to the host.
pub const ENV_TOKEN: &str = "KOYO_BROWSER_TOKEN";
/// The environment variable with the folder for CEF's caches and profiles.
pub const ENV_PROFILE_ROOT: &str = "KOYO_BROWSER_PROFILES";

/// The first frame on a connection, from the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    /// The app's [`PROTOCOL`].
    pub protocol: u32,
    /// The token the host was started with.
    pub token: String,
    /// Whether the app can ask the system for passkeys: it is signed with Apple's
    /// browser-passkey entitlement. Without it the host turns a page's passkey requests down
    /// at once, so the page offers its other ways of signing in.
    #[serde(default)]
    pub passkeys: bool,
}

/// A pane, by the app's id for it.
pub type PaneId = u64;

///
/// # Errors
/// When the message is larger than [`MAX_FRAME`] or the writer fails.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
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

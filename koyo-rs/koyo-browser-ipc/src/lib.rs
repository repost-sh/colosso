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
use serde::de::DeserializeOwned;
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

/// Writes `message` as one frame of JSON, in one write.
///
/// # Errors
/// When the message is larger than [`koyo_frame::MAX_FRAME`] or the writer fails.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    koyo_frame::write(writer, |body| serde_json::to_writer(body, message))
}

/// Reads one frame. `Ok(None)` is a clean end of stream between frames. A
/// link's reader reads a stream of them through [`koyo_frame::frames`], which
/// buffers it.
///
/// # Errors
/// When the stream ends inside a frame, a frame is larger than
/// [`koyo_frame::MAX_FRAME`], or its body is not the expected message.
pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<Option<T>> {
    koyo_frame::read(reader, |body| serde_json::from_slice(body))
}

#[cfg(test)]
mod tests;

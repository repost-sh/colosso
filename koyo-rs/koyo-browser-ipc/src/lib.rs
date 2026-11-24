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
mod downloads;
mod events;
mod extensions;
mod input;
mod passkeys;
mod requests;
mod session;
mod tone;

pub use devtools::{DevToolsAsk, DevToolsDock};
pub use dialogs::{Answer, Dialog, FileMode, MenuEntry, MenuTarget, Permission};
pub use downloads::{Download, DownloadAct, DownloadState, Print};
pub use events::{Cursor, Framework, HostEvent, PageState, SiteSetting, Zoom};
pub use extensions::{
    Corner, Extension, ExtensionAction, ExtensionChange, ExtensionSource, ScreenRect,
};
pub use input::{Point, Viewport};
pub use passkeys::{
    Assertion, AuthenticatorSelection, Bytes, Created, Creation, CredentialDescriptor,
    CredentialParameters, Extensions, Got, PasskeyAnswer, PasskeyAsk, PasskeyError, Refusal,
    RelyingParty, User,
};
pub use requests::{AppRequest, CdpCaller, Edit, Go, ViewAction};
pub use session::{Scroll, Session, SessionEntry};
pub use tone::Tone;

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
/// 16: the engine says what the framework it loaded was built with ([`HostEvent::Ready`]'s
/// `framework`).
/// 17: what a page brings back across a restart, its history and scroll
/// ([`HostEvent::Session`], [`AppRequest::BringBack`]), and the zoom the engine keeps for its
/// site ([`PageState::zoom`]).
/// 18: what a page saves (`docs/browser.md` 16): its downloads, where each goes
/// ([`HostEvent::DownloadAsked`], [`AppRequest::Download`]) and how it stands
/// ([`HostEvent::Download`]), and the page printed or saved as a PDF ([`AppRequest::Print`],
/// [`HostEvent::PdfSaved`]).
/// 19: lighter traffic: a page says its tone itself as it changes ([`HostEvent::Tone`]) where
/// the app asked each second, its loading progress comes on its own and in steps
/// ([`HostEvent::Progress`]) where the whole [`PageState`] came for each percent, a page's icon
/// travels as base64 ([`Bytes`]), and a page's events reach the app only for the domains an
/// agent enabled ([`AppRequest::Cdp`]'s `caller`).
/// 20: an agent's commands and events in the sessions of a page's cross-origin frames
/// ([`AppRequest::Cdp`]'s and [`HostEvent::CdpEvent`]'s `session`), whose attaching it hears
/// once it asked for it (`Target.setAutoAttach`).
/// 21: an extension's popup is the engine's from the press to its closing: a press carries
/// the icon's rectangle ([`AppRequest::ExtensionAct`]'s `bounds`) where it carried a point,
/// and the pointer going down on the icon comes before it ([`AppRequest::ExtensionPointerDown`]);
/// and the app says of a page only what it alone knows of its sight, whether an agent keeps it
/// and whether its pane's window is seen ([`AppRequest::Sight`]), where it said whether it was on
/// screen, so a page a pane stops drawing leaves every window at once rather than once told.
/// 22: a page the person closes is asked first ([`AppRequest::Leave`]), and one the app closes
/// goes unasked ([`AppRequest::Close`]).
pub const PROTOCOL: u32 = 22;

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

//! The terminal as the app shows it: the client of the daemon, the VT thread per terminal
//! view, and the GPUI element that paints the frames it publishes.
//! See `docs/terminals.md` section 7.

mod client;
mod element;
mod glyphs;
mod input;
mod keys;
mod paint;
mod sprite;
mod view;
mod vt;

pub use client::{Attachment, Notice, Stream, TermClient, is_other_build};
pub use glyphs::Look;
pub use view::{
    CONTEXT, Copy, Paste, ScrollPageDown, ScrollPageUp, ScrollToBottom, ScrollToTop, Source,
    TerminalEvent, TerminalView,
};

/// Binds the terminal view's keys; call once when the app starts.
pub fn init(cx: &mut gpui_kit::App) {
    view::bind_keys(cx);
}

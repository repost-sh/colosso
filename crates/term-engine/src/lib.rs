//! The terminal engine both sides run: libghostty-vt behind one module, so nothing else
//! names the binding's types. The daemon feeds it headless for snapshots, queries and status;
//! the app feeds it on a VT thread and paints from the frames it publishes.
//! See `docs/terminals.md` section 6.
//!
//! An [`Engine`] is not `Send`: it lives on the thread that made it, and what leaves that
//! thread is owned data, a [`Frame`], a snapshot's bytes, an [`Effect`].

mod engine;
mod frame;
mod input;
mod semantic;

pub use engine::{Effect, Engine, Progress, Scroll, Size, TextFormat};
pub use frame::{
    Attrs, Cell, Cursor, CursorFlags, CursorShape, Dirty, Frame, Grapheme, Rgb, Row, RowKind,
    Scrollbar, Underline, Width,
};
pub use input::{
    Geometry, GridPoint, Key, KeyAction, KeyInput, Mods, MouseAction, MouseButton, MouseInput,
    OptionAsAlt, SelectUnit, paste_is_safe,
};
pub use semantic::{Mark, PromptScanner};

/// What the engine reports when a call into libghostty-vt fails. These are rare: feeding
/// bytes never fails, and the rest fail only on exhausted memory or a value out of range.
pub use ghostty_vt::Error;

/// The engine's result type.
pub type Result<T> = std::result::Result<T, Error>;

/// The `TERM` the engine's terminfo describes, when the daemon installs it for children.
pub const TERM: &str = ghostty_vt::terminfo::TERM;

/// The folder holding the compiled `xterm-ghostty` terminfo, written once per engine
/// version under the cache folder (or `GHOSTTY_VT_CACHE_DIR`), for a child's `TERMINFO`.
pub fn terminfo_dir() -> std::io::Result<std::path::PathBuf> {
    ghostty_vt::terminfo::dir()
}

/// The folder to hand a shell as `GHOSTTY_RESOURCES_DIR`, holding Ghostty's shell
/// integration scripts for bash, zsh, fish, elvish and nushell.
pub fn shell_integration_dir() -> std::io::Result<std::path::PathBuf> {
    ghostty_vt::shell_integration::resources_dir()
}

/// The Ghostty commit the engine was built from, for version checks between the daemon and
/// its clients: a snapshot only restores into the same engine.
pub const GHOSTTY_COMMIT: &str = ghostty_vt::ffi::GHOSTTY_COMMIT;

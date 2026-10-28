//! The changes view: a checkout's changed files and their diffs, beside the chat.

mod discard;
mod highlight;
mod line;
mod rows;
mod state;
mod tree;
mod view;
mod watch;

pub use state::Changes;
pub use view::{ChangesPanel, ChangesResize, ToggleChangesWidth, counts, init};

//! The changes view: a checkout's changed files and their diffs, beside the chat. The
//! checkout reads them, with what's here; the panel shows them.

mod discard;
mod file_view;
mod highlight;
mod line;
mod marks;
mod reread;
mod rows;
mod tree;
mod view;
mod watch;

pub use file_view::FileView;
pub use marks::{Mark, Marks};
pub use reread::{Read, Stale, read};
pub use view::{ChangesPanel, ToggleChangesWidth, counts};
pub use watch::{Touched, touched};

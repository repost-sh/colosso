//! The changes: a checkout's changed files, listed in the right panel beside the chat, and
//! their diffs in a workspace tab. The checkout reads them, with what's here; the panel
//! lists them and the tab shows them.

mod discard;
mod file_view;
mod highlight;
mod line;
mod marks;
mod open;
mod reread;
mod rows;
mod tree;
mod view;
mod watch;

pub use file_view::FileView;
pub use line::{CodeLine, Half, Look, number_width};
pub use marks::{Mark, Marks};
pub use reread::{Read, Stale, read};
pub use view::{ChangesEvent, ChangesPanel, ChangesTab, counts};
pub use watch::{Touched, touched};

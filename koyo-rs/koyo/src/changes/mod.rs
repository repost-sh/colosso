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
pub use highlight::{LineTokens, highlight_lines, token_colour};
pub use line::{
    CODE_LEFT, CODE_RIGHT, LINE_TINT, NUMBER_RIGHT, NUMBERS_TINT, Span, WORDS_TINT, number_width,
    spans,
};
pub use marks::{Mark, Marks};
pub use reread::{Read, Stale, read};
pub use view::{ChangesEvent, ChangesPanel, ChangesTab, counts};
pub use watch::{Touched, touched};

//! What the two sides of Koyo's agent desktop agree on (`docs/agent-desktop.md`): the link
//! between the app and the host (8), its frames, the shapes on the desktop, and key names (7).
//! No UI and no Apple frameworks, so all of it is tested here.

pub mod frame;
pub mod geometry;
pub mod keys;
pub mod link;

pub use frame::{MAX_FRAME, read_frame, write_frame};
pub use geometry::{Point, Rect, Size};

//! What Koyo's agent desktops agree on (`docs/agent-desktop.md`): the link between the app
//! and the host inside the agent session (8), the layout of areas on the agent screen (6), key
//! names (7.7), the session holder's requests (6) and the root helper's (5.4), and the places
//! on disk all of them meet (5.3). No UI and no Apple frameworks, so all of it is tested here.

pub mod frame;
pub mod helper;
pub mod holder;
pub mod keys;
pub mod layout;
pub mod link;
pub mod paths;

pub use frame::{MAX_FRAME, read_frame, write_frame};
pub use layout::{AREA, Layout, Point, SLOTS, Size, slot_origin};

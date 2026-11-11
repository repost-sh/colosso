//! What the two sides of a device in Koyo agree on (`docs/devices.md` 7): the link between the
//! app and the host of one device, a simulator or an iPhone or iPad. The framing and the shapes
//! are the agent desktop's (`koyo_desk_wire`); the messages are the device's own. No UI and no
//! Apple frameworks, so all of it is tested here.

pub mod ask;
pub mod input;
pub mod link;

/// The environment a host is started with: the descriptor its end of the link is on, and the
/// secret its first message must carry back.
pub const ENV_SOCKET_FD: &str = "KOYO_DEVICE_SOCKET_FD";
pub const ENV_TOKEN: &str = "KOYO_DEVICE_TOKEN";

pub use ask::{Act, Ask, Press, Spot};
pub use input::{Button, Edge, Finger, Input, Orientation};
pub use koyo_desk_wire::{MAX_FRAME, Point, Rect, Size, read_frame, write_frame};
pub use link::{
    Answer, App, Change, FromHost, LogLevel, Mark, Missing, PROTOCOL, PixelSize, Refusal,
    RefusalCode, RequestId, Screen, Source, State, Step, ToHost,
};

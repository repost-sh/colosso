//! What the app tells the host about a pane: its viewport, and the pointer, keys and
//! modifiers in it.
use serde::{Deserialize, Serialize};

/// A size in device pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixelSize {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// The view a pane is laid out at.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    /// The pane's size in logical pixels.
    pub width: u32,
    /// The pane's height in logical pixels.
    pub height: u32,
    /// Device pixels per logical pixel.
    pub scale: f32,
}

impl Viewport {
    /// The surface size the host renders at.
    #[must_use]
    pub fn pixels(&self) -> PixelSize {
        let scale = if self.scale.is_finite() && self.scale > 0. {
            self.scale
        } else {
            1.
        };
        let scaled = |v: u32| ((v as f32) * scale).round().max(1.) as u32;
        PixelSize {
            width: scaled(self.width),
            height: scaled(self.height),
        }
    }
}

/// Keyboard and mouse modifiers held during an event.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each modifier is held or not; the wire format names them one by one"
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    /// Shift.
    #[serde(default)]
    pub shift: bool,
    /// Control.
    #[serde(default)]
    pub control: bool,
    /// Option / Alt.
    #[serde(default)]
    pub alt: bool,
    /// Command / Windows / Super.
    #[serde(default)]
    pub platform: bool,
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    /// The primary button.
    Left,
    /// The middle button or wheel press.
    Middle,
    /// The secondary button.
    Right,
}

/// What a mouse event is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseKind {
    /// The pointer moved, with `button` held if any.
    Move,
    /// A button went down.
    Down,
    /// A button went up.
    Up,
    /// The pointer left the pane.
    Leave,
}

/// What a key event is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyKind {
    /// A key went down.
    Down,
    /// A key went up.
    Up,
    /// Text the key produced.
    Char,
}

/// A position in a pane, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Left edge offset.
    pub x: f32,
    /// Top edge offset.
    pub y: f32,
}

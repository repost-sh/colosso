//! What the app tells the host about a pane: its viewport, and a point in it.
use serde::{Deserialize, Serialize};

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

/// The largest side a page is drawn at, in device pixels.
pub const MAX_SIDE: f32 = 16384.;

impl Viewport {
    /// The most logical pixels a side has at `scale` for the page to stay within [`MAX_SIDE`]
    /// device pixels; `scale` counts as 1 where it's none (not positive, or not a number).
    #[must_use]
    pub fn max_side(scale: f32) -> u32 {
        let scale = if scale.is_finite() && scale > 0. { scale } else { 1. };
        (MAX_SIDE / scale).floor() as u32
    }
}

/// A position in a pane, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Left edge offset.
    pub x: f32,
    /// Top edge offset.
    pub y: f32,
}

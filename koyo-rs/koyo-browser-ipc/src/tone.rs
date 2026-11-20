//! How light a page is, for the app's glass over it, and what its canvas is painted, for
//! what's behind it: the page works it out itself, as it first shows and whenever something
//! that could change it happens, and the host says it only when it changed
//! ([`crate::HostEvent::Tone`]).
use serde::{Deserialize, Serialize};

/// A page's tone: the lightness of the opaque background at each point of a grid over its
/// viewport, and the colour of its canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tone {
    /// The relative luminance, 0 to 1, at each point of a [`Tone::ACROSS`] by [`Tone::DOWN`]
    /// grid over the viewport, row by row: where no element paints one, the body's, else white.
    pub lightness: Vec<f32>,
    /// The canvas's colour as red, green and blue: the root's, else the body's, else white.
    pub canvas: [u8; 3],
}

impl Tone {
    /// How many points across the page are looked at.
    pub const ACROSS: usize = 12;
    /// How many points down.
    pub const DOWN: usize = 8;
}

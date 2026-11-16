//! The backdrop behind a chat (`docs/chat-backdrop.md` 6): whether it shows, how far the
//! wallpaper is dimmed, the dots over it, the blur it fades into, and the quality its picture is
//! kept at. Each slider keeps the value of its stop, as the other sliders keep theirs.

use serde::{Deserialize, Serialize};

use crate::chat_backdrop::Blocks;

/// How a chat's backdrop looks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatBackdrop {
    pub on: bool,
    /// How far the wallpaper is dimmed towards the card's colour, from 0 to 1.
    pub dim: f32,
    /// How wide a dot is, in points.
    pub dot_size: f32,
    /// From 0, sparse, to 1, dense: how close together the dots are and how dark.
    pub density: f32,
    pub blur: bool,
    /// How far the blur spreads, in points: a Gaussian's sigma, as CSS's `blur()` takes.
    pub blur_strength: f32,
    /// Where down the pane the blur starts fading in, from 0 at its top to 1 at its bottom.
    pub blur_start: f32,
    pub quality: Quality,
}

impl Default for ChatBackdrop {
    fn default() -> Self {
        ChatBackdrop {
            on: true,
            dim: 0.6,
            dot_size: 1.,
            density: 0.5,
            blur: true,
            blur_strength: 24.,
            blur_start: 0.5,
            quality: Quality::default(),
        }
    }
}

/// How a screen's picture is kept on the GPU: always compressed to ASTC, in blocks of 4 × 4 or
/// 8 × 8 pixels, at the screen's resolution or half of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Astc8Half,
    Astc4Half,
    #[default]
    Astc8Full,
    Astc4Full,
}

impl Quality {
    pub fn blocks(self) -> Blocks {
        match self {
            Quality::Astc8Half | Quality::Astc8Full => Blocks::Eight,
            Quality::Astc4Half | Quality::Astc4Full => Blocks::Four,
        }
    }

    /// The picture's pixels to the screen's: 1, or ½ for half.
    pub fn resolution(self) -> f64 {
        match self {
            Quality::Astc8Half | Quality::Astc4Half => 0.5,
            Quality::Astc8Full | Quality::Astc4Full => 1.,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ChatBackdrop;

    #[test]
    fn it_is_kept_as_the_doc_shows_it() {
        let json = serde_json::to_value(ChatBackdrop::default()).unwrap();
        let expected = serde_json::json!({
            "on": true,
            "dim": 0.6,
            "dot_size": 1.0,
            "density": 0.5,
            "blur": true,
            "blur_strength": 24.0,
            "blur_start": 0.5,
            "quality": "astc8_full",
        });
        // f32s widen to f64s that print a little off: compare as the file would read back.
        let back: ChatBackdrop = serde_json::from_value(expected).unwrap();
        assert_eq!(back, ChatBackdrop::default());
        assert_eq!(json["quality"], "astc8_full");
        let partial: ChatBackdrop = serde_json::from_str(r#"{"on": false}"#).unwrap();
        assert_eq!(
            partial,
            ChatBackdrop {
                on: false,
                ..ChatBackdrop::default()
            },
            "what the file leaves out is the default"
        );
    }
}

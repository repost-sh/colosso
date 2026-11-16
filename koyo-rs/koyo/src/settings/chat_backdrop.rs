//! The backdrop behind a chat (`docs/chat-backdrop.md` 6): whether it shows, how strongly, the
//! dither's cells, its fade, and the quality its picture is kept at. The look is Monodrive's
//! Dither effect, whose defaults these are. Each slider keeps the value of its stop, as the
//! other sliders keep theirs.

use serde::{Deserialize, Serialize};

use crate::chat_backdrop::Blocks;

/// How a chat's backdrop looks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatBackdrop {
    pub on: bool,
    /// How much of the dithered wallpaper shows over the card, from 0 to 1.
    pub opacity: f32,
    /// How many screen pixels a cell of the dither is a side.
    pub cell: f32,
    /// How many cells are lit: 1 as Monodrive lights them, more for more.
    pub density: f32,
    pub fade: bool,
    /// Where down the pane it starts fading out, and where it's gone, from 0 at the pane's top
    /// to 1 at its bottom.
    pub fade_start: f32,
    pub fade_end: f32,
    pub quality: Quality,
}

impl Default for ChatBackdrop {
    fn default() -> Self {
        ChatBackdrop {
            on: true,
            opacity: 0.24,
            cell: 2.,
            density: 1.,
            fade: true,
            fade_start: 0.18,
            fade_end: 0.72,
            quality: Quality::default(),
        }
    }
}

impl ChatBackdrop {
    /// The stops of the Opacity slider, from less to more.
    pub const OPACITY_STOPS: [f32; 5] = [0.12, 0.18, 0.24, 0.35, 0.5];
    /// The stops of the Cell size slider, in screen pixels, from fine to coarse.
    pub const CELL_STOPS: [f32; 4] = [1., 2., 3., 4.];
    /// The stops of the Density slider, from sparse to dense.
    pub const DENSITY_STOPS: [f32; 5] = [0.6, 0.8, 1., 1.25, 1.5];
    /// The stops of the Fade starts slider, from high in the pane to low.
    pub const FADE_START_STOPS: [f32; 5] = [0., 0.09, 0.18, 0.3, 0.45];
    /// The stops of the Fade ends slider, from high in the pane to low: each below the lowest
    /// start, so the fade always ends below where it starts.
    pub const FADE_END_STOPS: [f32; 5] = [0.5, 0.6, 0.72, 0.85, 1.];
    /// The stops of the Quality slider, one for each of [`Quality::ALL`].
    pub const QUALITY_STOPS: [f32; 3] = [0., 1., 2.];

    /// Where the fade starts and ends, if it fades: in order, whatever the file says.
    pub fn fade(&self) -> Option<[f32; 2]> {
        let start = self.fade_start.clamp(0., 1.);
        let end = self.fade_end.clamp(0., 1.).max(start);
        self.fade.then_some([start, end])
    }
}

/// How a screen's picture is kept on the GPU: always compressed to ASTC, in blocks of 8 × 8
/// pixels at half the screen's resolution or the whole, or of 4 × 4 at the whole. 4 × 4 at half
/// took as much memory as 8 × 8 at the whole and looked the same, so it was dropped; a file that
/// still names it, or any other, reads as the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Astc8Half,
    Astc4Full,
    /// Last, as what a file names that isn't one of these reads as this.
    #[default]
    #[serde(other)]
    Astc8Full,
}

impl Quality {
    /// From the smallest to the sharpest.
    pub const ALL: [Quality; 3] = [Quality::Astc8Half, Quality::Astc8Full, Quality::Astc4Full];

    pub fn blocks(self) -> Blocks {
        match self {
            Quality::Astc8Half | Quality::Astc8Full => Blocks::Eight,
            Quality::Astc4Full => Blocks::Four,
        }
    }

    /// The picture's pixels to the screen's: 1, or ½ for half.
    pub fn resolution(self) -> f64 {
        match self {
            Quality::Astc8Half => 0.5,
            Quality::Astc8Full | Quality::Astc4Full => 1.,
        }
    }

    /// The bytes the picture of a screen `width` by `height` pixels takes at this quality.
    pub fn bytes(self, width: u32, height: u32) -> usize {
        let side = |pixels: u32| (f64::from(pixels) * self.resolution()).round() as u32;
        self.blocks().bytes(side(width), side(height))
    }

    /// Its place along the Quality slider.
    pub fn stop(self) -> f32 {
        Quality::ALL
            .iter()
            .position(|quality| *quality == self)
            .unwrap_or_default() as f32
    }

    /// The quality at `stop` along the Quality slider.
    pub fn at(stop: f32) -> Quality {
        let index = stop.round().clamp(0., (Quality::ALL.len() - 1) as f32) as usize;
        Quality::ALL[index]
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatBackdrop, Quality};

    #[test]
    fn it_is_kept_as_the_doc_shows_it() {
        let json = serde_json::to_value(ChatBackdrop::default()).unwrap();
        let expected = serde_json::json!({
            "on": true,
            "opacity": 0.24,
            "cell": 2.0,
            "density": 1.0,
            "fade": true,
            "fade_start": 0.18,
            "fade_end": 0.72,
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

    #[test]
    fn the_fade_ends_below_where_it_starts_whatever_the_file_says() {
        let lowest_end = ChatBackdrop::FADE_END_STOPS[0];
        let starts = ChatBackdrop::FADE_START_STOPS;
        assert!(
            starts.iter().all(|start| *start < lowest_end),
            "every stop's pair"
        );
        let backwards = ChatBackdrop {
            fade_start: 0.9,
            fade_end: 0.2,
            ..ChatBackdrop::default()
        };
        let [start, end] = backwards.fade().unwrap();
        assert!(end >= start, "{start} to {end}");
        let off = ChatBackdrop {
            fade: false,
            ..ChatBackdrop::default()
        };
        assert_eq!(off.fade(), None);
    }

    #[test]
    fn each_quality_stop_takes_the_memory_section_3_1_names() {
        // In tenths of a megabyte.
        let tenths = |quality: Quality| (quality.bytes(5120, 2880) as f64 / 1e5).round() as u32;
        assert_eq!(Quality::ALL.map(tenths), [9, 37, 147]);
        for quality in Quality::ALL {
            assert_eq!(Quality::at(quality.stop()), quality);
        }
        assert_eq!(
            Quality::at(9.),
            Quality::Astc4Full,
            "past the end, the sharpest"
        );
        let dropped: ChatBackdrop =
            serde_json::from_str(r#"{"quality": "astc4_half", "opacity": 0.5}"#).unwrap();
        assert_eq!(
            dropped.quality,
            Quality::default(),
            "a dropped stop reads as the default"
        );
        assert_eq!(
            dropped.opacity.to_bits(),
            0.5f32.to_bits(),
            "and the rest as written"
        );
    }
}

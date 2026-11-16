//! The dot screen over a chat's backdrop: dots in a staggered grid, each row's between the last
//! row's, darker than the picture under them, in a tile of whole rows and columns of the grid.
//! The tile is repeated across the pane one tile pixel to a screen pixel, from the screen's own
//! top left, so the grid holds still as the window moves and two chats side by side show one.
//! It's a screen over the picture, not a dither of it: every dot is the same.

use std::sync::Arc;

use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};

/// About how wide and tall a tile is, in pixels: large, so few tiles cover a pane.
const TILE: u32 = 256;
/// How many samples a side a pixel's coverage of a dot is measured with.
const SAMPLES: u32 = 4;

/// The grid a tile repeats, in a screen's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// How far apart two dots of a row are, even, so the next row's sit half way between them
    /// on whole pixels; and how far apart two rows are.
    pub pitch: u32,
    pub row: u32,
    /// How wide a dot is.
    pub diameter: f32,
    /// How dark a dot is at its middle, from 0 to 1.
    pub darkness: f32,
}

impl Grid {
    /// The grid of dots `dot_size` points wide on a screen of `scale` pixels to the point, at a
    /// `density` from 0, sparse, to 1, dense: denser, the dots are closer together and darker.
    pub fn new(dot_size: f32, density: f32, scale: f32) -> Grid {
        let density = density.clamp(0., 1.);
        let diameter = (dot_size * scale).max(1.);
        // From five dots' widths apart to two and a half.
        let spacing = diameter * (5. - 2.5 * density);
        let pitch = ((spacing / 2.).round() as u32).max(1) * 2;
        // Rows as far apart as a triangle's height, so each dot is as far from all six round it.
        let row = ((pitch as f32 * 3f32.sqrt() / 2.).round() as u32).max(1);
        Grid {
            pitch,
            row,
            diameter,
            darkness: 0.18 + 0.32 * density,
        }
    }

    /// How wide and tall the tile is: whole dots across, whole pairs of rows down.
    pub fn tile(self) -> (u32, u32) {
        let across = (TILE / self.pitch).max(1);
        let down = (TILE / (self.row * 2)).max(1);
        (across * self.pitch, down * self.row * 2)
    }

    /// How much of the pixel at `x`, `y` of the tile the dots cover, from 0 to 1. Row `n` of
    /// the grid is `n` rows down from the tile's top, and the odd rows are half a pitch over.
    fn coverage(self, x: u32, y: u32) -> f32 {
        let (pitch, row) = (self.pitch as f32, self.row as f32);
        let radius = self.diameter / 2.;
        // A dot an odd number of pixels wide sits on a pixel's middle, an even one on a corner,
        // so either fills whole pixels.
        let phase = if (self.diameter.round() as u32) % 2 == 1 {
            0.5
        } else {
            0.
        };
        let at = |pixel: u32, sample: u32| pixel as f32 + (sample as f32 + 0.5) / SAMPLES as f32;
        let mut covered = 0;
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let (px, py) = (at(x, sx), at(y, sy));
                let nearest = ((py - phase) / row).round() as i64;
                let inside = (nearest - 1..=nearest + 1).any(|line| {
                    let cy = line as f32 * row + phase;
                    let shift = if line.rem_euclid(2) == 1 { pitch / 2. } else { 0. };
                    let cx = ((px - phase - shift) / pitch).round() * pitch + shift + phase;
                    (px - cx).hypot(py - cy) <= radius
                });
                covered += u32::from(inside);
            }
        }
        covered as f32 / (SAMPLES * SAMPLES) as f32
    }
}

/// The tile of `grid`, as GPUI draws it: black, see-through but for the dots.
pub fn tile(grid: Grid) -> Arc<RenderImage> {
    let (width, height) = grid.tile();
    let image = RgbaImage::from_fn(width, height, |x, y| {
        let alpha = grid.coverage(x, y) * grid.darkness;
        image::Rgba([0, 0, 0, (alpha * 255.).round() as u8])
    });
    Arc::new(RenderImage::new(vec![Frame::new(image)]))
}

#[cfg(test)]
mod tests {
    use super::Grid;

    #[test]
    fn denser_dots_are_closer_together_and_darker() {
        let sparse = Grid::new(1., 0., 2.);
        let dense = Grid::new(1., 1., 2.);
        assert!(dense.pitch < sparse.pitch);
        assert!(dense.darkness > sparse.darkness);
        assert_eq!(sparse.pitch % 2, 0, "the next row's dots sit on whole pixels");
        assert_eq!(dense.pitch % 2, 0);
    }

    #[test]
    fn a_tile_holds_whole_dots_and_whole_pairs_of_rows() {
        for (size, density, scale) in [(1., 0.5, 2.), (2., 0.2, 1.), (0.5, 1., 2.), (3., 0., 2.)] {
            let grid = Grid::new(size, density, scale);
            let (width, height) = grid.tile();
            assert_eq!(width % grid.pitch, 0, "{grid:?}");
            assert_eq!(height % (grid.row * 2), 0, "{grid:?}");
        }
    }

    #[test]
    fn a_dot_of_two_pixels_fills_two_by_two_and_each_row_sits_between_the_last() {
        let grid = Grid::new(1., 0.5, 2.);
        assert_eq!((grid.pitch, grid.row), (8, 7));
        let covered: Vec<(u32, u32)> = (0..grid.row * 2)
            .flat_map(|y| (0..grid.pitch).map(move |x| (x, y)))
            .filter(|&(x, y)| grid.coverage(x, y) > 0.5)
            .collect();
        // Row 0's dots on the tile's corners, a quarter of each in it; row 1's half way along.
        let expected = [(0, 0), (7, 0), (3, 6), (4, 6), (3, 7), (4, 7), (0, 13), (7, 13)];
        assert_eq!(covered, expected);
    }

    #[test]
    fn a_dot_of_an_odd_width_sits_on_a_pixels_middle() {
        let grid = Grid::new(1., 0.5, 1.);
        assert!(grid.coverage(0, 0) >= 0.75, "the dot's middle pixel, all but its corners");
        assert!(grid.coverage(1, 0) < 0.25, "the next one, barely");
    }
}

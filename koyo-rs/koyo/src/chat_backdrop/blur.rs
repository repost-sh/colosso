//! The blur the bottom of a chat's backdrop fades into: a copy of the dimmed picture a tenth of
//! the screen's size, which a blur leaves smooth enough to be stretched over the screen, blurred
//! by three box blurs one after another, which come within a few percent of a Gaussian's.

use std::sync::Arc;

use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};

/// How much smaller than the screen's pixels the blurred copy is, a side.
pub const SHRINK: f64 = 10.;

/// A copy of a picture, four bytes a pixel, blue first, opaque.
#[derive(Clone)]
pub struct Small {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Small {
    /// The copy blurred as a Gaussian `sigma` of its own pixels wide would blur it.
    pub fn blurred(&self, sigma: f32) -> Small {
        let (width, height) = (self.width as usize, self.height as usize);
        let mut pixels = self.pixels.clone();
        let mut spare = vec![0; pixels.len()];
        for radius in box_radii(sigma) {
            pass(&pixels, &mut spare, width, height, radius, Axis::Across);
            pass(&spare, &mut pixels, width, height, radius, Axis::Down);
        }
        Small {
            width: self.width,
            height: self.height,
            pixels,
        }
    }

    /// The copy as an image GPUI draws.
    pub fn image(self) -> Option<Arc<RenderImage>> {
        let image = RgbaImage::from_raw(self.width, self.height, self.pixels)?;
        Some(Arc::new(RenderImage::new(vec![Frame::new(image)])))
    }
}

/// The radii of three box blurs that together blur as a Gaussian `sigma` wide: Kovesi's widths,
/// the two smaller then the larger, so the boxes' variances add up to the Gaussian's.
fn box_radii(sigma: f32) -> [usize; 3] {
    if sigma <= 0. {
        return [0; 3];
    }
    let passes = 3.;
    let ideal = (12. * sigma * sigma / passes + 1.).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let upper = lower + 2;
    let l = lower as f32;
    let smaller = ((12. * sigma * sigma - passes * l * l - 4. * passes * l - 3. * passes)
        / (-4. * l - 4.))
        .round()
        .clamp(0., passes) as usize;
    let radius = |width: i64| (width as usize - 1) / 2;
    let mut radii = [radius(upper); 3];
    for slot in radii.iter_mut().take(smaller) {
        *slot = radius(lower);
    }
    radii
}

#[derive(Clone, Copy)]
enum Axis {
    Across,
    Down,
}

/// One box blur of `radius` along `axis`, from `from` into `into`, the picture's edge pixels
/// standing in for what lies past it.
fn pass(from: &[u8], into: &mut [u8], width: usize, height: usize, radius: usize, axis: Axis) {
    let (lines, length, step, stride) = match axis {
        Axis::Across => (height, width, 4, width * 4),
        Axis::Down => (width, height, width * 4, 4),
    };
    let span = (2 * radius + 1) as u32;
    for line in 0..lines {
        let start = line * stride;
        let at = |index: isize| start + index.clamp(0, length as isize - 1) as usize * step;
        let mut sums = [0u32; 4];
        for index in -(radius as isize)..=radius as isize {
            for (channel, sum) in sums.iter_mut().enumerate() {
                *sum += u32::from(from[at(index) + channel]);
            }
        }
        for index in 0..length as isize {
            let out = start + index as usize * step;
            for (channel, sum) in sums.iter().enumerate() {
                into[out + channel] = ((sum + span / 2) / span) as u8;
            }
            let (leaving, coming) = (at(index - radius as isize), at(index + radius as isize + 1));
            for (channel, sum) in sums.iter_mut().enumerate() {
                *sum = *sum + u32::from(from[coming + channel]) - u32::from(from[leaving + channel]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Small, box_radii};

    /// A picture `width` by `height`, black but for a white column at `column`.
    fn column(width: u32, height: u32, column: u32) -> Small {
        let pixels = (0..height)
            .flat_map(|_| 0..width)
            .flat_map(|x| {
                let shade = if x == column { 255 } else { 0 };
                [shade, shade, shade, 255]
            })
            .collect();
        Small {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn three_boxes_blur_as_wide_as_the_gaussian_asked_for() {
        // From the softest blur at 2× (8 pt, 1.6 of the copy's pixels) to the strongest. Boxes
        // a whole number of pixels wide can't come as close to a narrow Gaussian as to a wide.
        for (sigma, within) in [(1., 0.2), (1.6, 0.15), (2.5, 0.1), (4.8, 0.1), (9.6, 0.1)] {
            let radii = box_radii(sigma);
            let variance: f32 = radii
                .iter()
                .map(|radius| {
                    let width = (2 * radius + 1) as f32;
                    (width * width - 1.) / 12.
                })
                .sum();
            let error = (variance.sqrt() - sigma).abs() / sigma;
            assert!(error < within, "sigma {sigma}: {radii:?} blur {}", variance.sqrt());
        }
        assert_eq!(box_radii(0.), [0; 3], "no blur");
    }

    #[test]
    fn a_blur_spreads_a_line_and_keeps_its_light() {
        let line = column(41, 3, 20);
        let blurred = line.blurred(3.);
        let row: Vec<u32> = (0..41)
            .map(|x| u32::from(blurred.pixels[(41 + x) * 4]))
            .collect();
        let total: u32 = row.iter().sum();
        assert!((240..=270).contains(&total), "the light is kept: {total}");
        assert!(row[20] > row[17] && row[17] > row[14], "it falls off from the middle");
        assert_eq!(row[20], *row.iter().max().unwrap());
        assert!(blurred.pixels.chunks(4).all(|pixel| pixel[3] == 255), "still opaque");
    }

    #[test]
    fn the_edges_hold_their_colour() {
        let white = Small {
            width: 5,
            height: 5,
            pixels: vec![255; 5 * 5 * 4],
        };
        assert!(white.blurred(4.).pixels.iter().all(|&channel| channel == 255));
    }
}

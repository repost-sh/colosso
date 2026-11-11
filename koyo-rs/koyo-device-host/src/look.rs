//! Looking at the picture (`docs/devices.md` 6): a grid of its points sampled to tell whether an
//! act changed it and whether it has gone still, and a screenshot as PNG. Both read a slot of the
//! ring the host holds while it reads, so no frame is copied into it meanwhile.

use koyo_device_wire::{Change, Rect, Size};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};

/// How far apart the sampled points are, in points: a switch flipped is still seen.
const STEP_POINTS: f64 = 4.;
/// Below this share of the sampled points, a change is small: a switch, a cursor, or on a real
/// device the stream's own noise. Above it, large.
const SMALL: f64 = 0.01;

/// A grid of the picture's pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    columns: usize,
    rows: usize,
    /// Points from one sampled pixel to the next.
    step: f64,
    pixels: Vec<u32>,
}

/// What differs between two samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diff {
    pub differing: usize,
    pub total: usize,
    pub region: Option<Rect>,
}

impl Diff {
    pub fn change(&self) -> Change {
        if self.differing == 0 {
            Change::None
        } else if (self.differing as f64) < self.total as f64 * SMALL {
            Change::Small
        } else {
            Change::Large
        }
    }
}

/// Reads `surface` locked for reading: its width, height, bytes a row and pixels.
fn read<R>(
    surface: &IOSurfaceRef,
    reader: impl FnOnce(usize, usize, usize, &[u8]) -> R,
) -> Option<R> {
    // SAFETY: locked for reading, the base address holds `height` rows of `stride` bytes; the
    // slice lives only inside the lock.
    unsafe {
        if surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) != 0 {
            return None;
        }
        let (width, height, stride) = (surface.width(), surface.height(), surface.bytes_per_row());
        let bytes = std::slice::from_raw_parts(
            surface.base_address().as_ptr().cast::<u8>(),
            stride * height,
        );
        let result = (width * 4 <= stride).then(|| reader(width, height, stride, bytes));
        surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        result
    }
}

/// Samples `surface`, whose pixels are `scale` a point.
pub fn sample(surface: &IOSurfaceRef, scale: f64) -> Option<Sample> {
    let every = ((STEP_POINTS * scale).round() as usize).max(1);
    read(surface, |width, height, stride, bytes| {
        let (columns, rows) = (width.div_ceil(every), height.div_ceil(every));
        let mut pixels = Vec::with_capacity(columns * rows);
        for y in (0..height).step_by(every) {
            let row = &bytes[y * stride..y * stride + width * 4];
            for x in (0..width).step_by(every) {
                let p = &row[x * 4..x * 4 + 4];
                pixels.push(u32::from_le_bytes([p[0], p[1], p[2], 0]));
            }
        }
        Sample {
            columns,
            rows,
            step: every as f64 / scale,
            pixels,
        }
    })
}

/// The widest and tallest a text cursor is, in points.
const CARET: (f64, f64) = (8., 60.);

/// Whether a change within `region` is no more than a text cursor blinking.
pub fn is_caret(region: Rect) -> bool {
    region.width <= CARET.0 && region.height <= CARET.1
}

/// What differs from `before` to `after`: a point differs when a colour of it moved by more
/// than `tolerance` (0 for a simulator, whose pixels are exact).
pub fn compare(before: &Sample, after: &Sample, tolerance: u8) -> Diff {
    if (before.columns, before.rows) != (after.columns, after.rows) {
        let total = after.pixels.len();
        return Diff {
            differing: total,
            total,
            region: Some(Rect {
                x: 0.,
                y: 0.,
                width: after.columns as f64 * after.step,
                height: after.rows as f64 * after.step,
            }),
        };
    }
    let apart = |a: u32, b: u32| {
        let (a, b) = (a.to_le_bytes(), b.to_le_bytes());
        a.iter()
            .zip(&b)
            .take(3)
            .any(|(x, y)| x.abs_diff(*y) > tolerance)
    };
    let (mut differing, mut low, mut high) = (0, (usize::MAX, usize::MAX), (0, 0));
    for (index, (a, b)) in before.pixels.iter().zip(&after.pixels).enumerate() {
        if apart(*a, *b) {
            differing += 1;
            let (x, y) = (index % after.columns, index / after.columns);
            low = (low.0.min(x), low.1.min(y));
            high = (high.0.max(x), high.1.max(y));
        }
    }
    let region = (differing > 0).then(|| Rect {
        x: low.0 as f64 * after.step,
        y: low.1 as f64 * after.step,
        width: (high.0 - low.0 + 1) as f64 * after.step,
        height: (high.1 - low.1 + 1) as f64 * after.step,
    });
    Diff {
        differing,
        total: after.pixels.len(),
        region,
    }
}

/// A PNG of `surface`, whose pixels are `native` a point, at `scale` pixels a point (no more
/// than native), and its size in points.
pub fn screenshot(
    surface: &IOSurfaceRef,
    native: f64,
    scale: u8,
) -> Result<(Vec<u8>, Size), String> {
    let wanted = f64::from(scale.clamp(1, 3)).min(native);
    let factor = native / wanted;
    read(surface, |width, height, stride, bytes| {
        let (out_w, out_h) = (
            ((width as f64 / factor).round() as usize).max(1),
            ((height as f64 / factor).round() as usize).max(1),
        );
        let mut rgb = Vec::with_capacity(out_w * out_h * 3);
        for oy in 0..out_h {
            let (y0, y1) = span(oy, factor, height);
            for ox in 0..out_w {
                let (x0, x1) = span(ox, factor, width);
                let mut sum = [0u32; 3];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = &bytes[y * stride + x * 4..y * stride + x * 4 + 3];
                        // BGRA in memory: blue, green, red.
                        sum[0] += u32::from(p[2]);
                        sum[1] += u32::from(p[1]);
                        sum[2] += u32::from(p[0]);
                    }
                }
                let count = u32::try_from((y1 - y0) * (x1 - x0)).unwrap_or(1).max(1);
                rgb.extend(
                    sum.iter()
                        .map(|c| u8::try_from(c / count).unwrap_or(u8::MAX)),
                );
            }
        }
        let size = Size {
            width: width as f64 / native,
            height: height as f64 / native,
        };
        encode(&rgb, out_w, out_h).map(|png| (png, size))
    })
    .ok_or("the picture couldn't be read")?
}

/// The source pixels output pixel `index` covers, at `factor` source pixels each.
fn span(index: usize, factor: f64, limit: usize) -> (usize, usize) {
    let start = ((index as f64 * factor).floor() as usize).min(limit.saturating_sub(1));
    let end = (((index + 1) as f64 * factor).ceil() as usize).clamp(start + 1, limit);
    (start, end)
}

fn encode(rgb: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    let (w, h) = (
        u32::try_from(width).map_err(|error| error.to_string())?,
        u32::try_from(height).map_err(|error| error.to_string())?,
    );
    let mut encoder = png::Encoder::new(&mut png, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(rgb)
        .map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(columns: usize, rows: usize, pixels: Vec<u32>) -> Sample {
        Sample {
            columns,
            rows,
            step: 4.,
            pixels,
        }
    }

    #[test]
    fn a_cursor_blinking_is_no_change_to_wait_for() {
        let rect = |width, height| Rect {
            x: 10.,
            y: 10.,
            width,
            height,
        };
        assert!(is_caret(rect(4., 24.)));
        assert!(!is_caret(rect(40., 24.)));
        assert!(!is_caret(rect(4., 200.)));
    }

    #[test]
    fn nothing_differing_is_no_change_and_a_corner_is_its_region() {
        let a = sample(10, 10, vec![0; 100]);
        assert_eq!(compare(&a, &a, 0).change(), Change::None);
        let mut pixels = vec![0; 100];
        pixels[99] = 0x00FF_FFFF;
        let diff = compare(&a, &sample(10, 10, pixels), 0);
        assert_eq!(diff.differing, 1);
        assert_eq!(
            diff.region,
            Some(Rect {
                x: 36.,
                y: 36.,
                width: 4.,
                height: 4.
            })
        );
        // One point in a hundred is the line between small and large.
        assert_eq!(diff.change(), Change::Large);
        let mut many = vec![0; 1000];
        many[5] = 1 << 16;
        assert_eq!(
            compare(&sample(100, 10, vec![0; 1000]), &sample(100, 10, many), 0).change(),
            Change::Small
        );
    }

    #[test]
    fn a_device_s_noise_under_its_tolerance_is_no_change() {
        let a = sample(2, 1, vec![0x0010_1010, 0]);
        let b = sample(2, 1, vec![0x0012_1210, 0]);
        assert_eq!(compare(&a, &b, 4).differing, 0);
        assert_eq!(compare(&a, &b, 0).differing, 1);
    }

    #[test]
    fn a_screenshot_s_pixels_cover_the_source_without_gaps() {
        assert_eq!(span(0, 3., 1206), (0, 3));
        assert_eq!(span(401, 3., 1206), (1203, 1206));
        assert_eq!(span(0, 1.5, 10), (0, 2));
        assert_eq!(span(1, 1.5, 10), (1, 3));
    }
}

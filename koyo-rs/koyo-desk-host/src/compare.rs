//! How a frame differs from the one before it (`docs/agent-desktop.md` 6): a window that
//! isn't showing is delivered whole whether or not it changed, so equal frames are told from
//! changed ones here, and a text cursor's blink from a change worth waiting out.

use crate::sys;

/// How a frame differs from the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Same,
    /// No more than a text cursor's worth.
    Small,
    Big,
}

/// The rows of the last frame that are compared: one in every few, as the caller steps.
#[derive(Default)]
pub struct Sampled {
    size: (usize, usize),
    rows: Vec<u8>,
}

impl Sampled {
    /// Compares `surface` with the frame before it and takes its place. `cursor` is the most
    /// square pixels that still count as small.
    pub fn compare(&mut self, surface: sys::Ref, step: usize, cursor: f64) -> Change {
        const READ_ONLY: u32 = 1;
        // SAFETY: the surface is live for the callback; locked for reading, its base address
        // holds `height` rows of `stride` bytes, of which `width * 4` are a row's pixels.
        unsafe {
            if sys::IOSurfaceLock(surface, READ_ONLY, std::ptr::null_mut()) != 0 {
                return Change::Big;
            }
            let (width, height) = (
                sys::IOSurfaceGetWidth(surface),
                sys::IOSurfaceGetHeight(surface),
            );
            let (stride, base) = (
                sys::IOSurfaceGetBytesPerRow(surface),
                sys::IOSurfaceGetBaseAddress(surface),
            );
            let row = width * 4;
            let change = if base.is_null() || row == 0 || row > stride {
                Change::Big
            } else {
                let lines = (0..height).step_by(step);
                let fresh = self.size != (width, height);
                if fresh {
                    self.size = (width, height);
                    self.rows = vec![0; lines.len() * row];
                }
                let mut area = 0usize;
                for (kept, line) in self.rows.chunks_exact_mut(row).zip(lines) {
                    let now = std::slice::from_raw_parts(base.add(line * stride), row);
                    if now != kept {
                        let first = now
                            .iter()
                            .zip(kept.iter())
                            .position(|(a, b)| a != b)
                            .unwrap_or(0);
                        let last = now
                            .iter()
                            .zip(kept.iter())
                            .rposition(|(a, b)| a != b)
                            .unwrap_or(row);
                        area += ((last - first) / 4 + 1) * step;
                        kept.copy_from_slice(now);
                    }
                }
                if fresh || area as f64 > cursor {
                    Change::Big
                } else if area == 0 {
                    Change::Same
                } else {
                    Change::Small
                }
            };
            sys::IOSurfaceUnlock(surface, READ_ONLY, std::ptr::null_mut());
            change
        }
    }
}

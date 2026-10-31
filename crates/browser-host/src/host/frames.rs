//! A pane's frames: the ring of shared surfaces it paints into, each GPU paint composed
//! into a free slot and announced, and the slots the app hands back.
use super::{Pane, send, with, with_pane};
use cef::{AcceleratedPaintInfo, Rect};
use colosso_browser_ipc::{self as ipc, HostEvent, PaneId, PixelSize};

/// How long paints at a size other than the pane's are dropped after a
/// resize, in case the page never paints at the exact size asked for.
#[cfg(target_os = "macos")]
const RESIZE_SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

#[cfg(target_os = "macos")]
impl Pane {
    /// Whether a page paint of `size` should be dropped. Right after a
    /// resize Chromium paints once more at the old size, with the new layout
    /// scaled into it between black bars; showing it flashes the pane.
    fn stale_paint(&mut self, size: PixelSize) -> bool {
        let Some(at) = self.resized else {
            return false;
        };
        let want = self.viewport.pixels();
        let near = |a: u32, b: u32| a.abs_diff(b) <= 1;
        if (near(size.width, want.width) && near(size.height, want.height))
            || at.elapsed() > RESIZE_SETTLE
        {
            self.resized = None;
            return false;
        }
        true
    }
}

/// A fresh ring at the pane's current pixel size, announced to the app.
pub(super) fn new_ring(pane: PaneId) {
    #[cfg(target_os = "macos")]
    {
        let made = with(|host| {
            let size = host.panes.get(&pane)?.viewport.pixels();
            let ring = host.gpu.as_ref()?.ring(crate::surfaces::capacity(size))?;
            let p = host.panes.get_mut(&pane)?;
            p.generation += 1;
            p.free = (0..ipc::RING).collect();
            p.missed = false;
            let generation = p.generation;
            let sent = host
                .surfaces
                .as_ref()
                .is_some_and(|s| crate::surfaces::send(s, pane, generation, &ring));
            host.panes.get_mut(&pane)?.ring = Some(ring);
            Some((generation, size, sent))
        })
        .flatten();
        match made {
            Some((generation, size, true)) => send(&HostEvent::Surfaces {
                pane,
                generation,
                size,
                slots: ipc::RING,
            }),
            _ => send(&HostEvent::Error {
                pane: Some(pane),
                message: "Could not share the page's surfaces.".into(),
            }),
        }
    }
}

pub(super) fn release(pane: PaneId, generation: u32, slot: u8) {
    let repaint = with_pane(pane, |p| {
        if generation != p.generation {
            return false;
        }
        if slot < ipc::RING && !p.free.contains(&slot) {
            p.free.push(slot);
        }
        std::mem::take(&mut p.missed)
    });
    if repaint == Some(true) {
        present(pane);
    }
}

pub(crate) fn logical_size(pane: PaneId) -> (i32, i32) {
    with_pane(pane, |p| {
        (
            i32::try_from(p.viewport.width.max(1)).unwrap_or(1),
            i32::try_from(p.viewport.height.max(1)).unwrap_or(1),
        )
    })
    .unwrap_or((1, 1))
}

pub(crate) fn scale(pane: PaneId) -> f32 {
    with_pane(pane, |p| p.viewport.scale)
        .filter(|s| s.is_finite() && *s > 0.)
        .unwrap_or(1.)
}

/// A GPU paint of the page or its popup: kept, composed into a free slot,
/// and announced to the app.
pub(crate) fn accelerated_frame(
    pane: PaneId,
    layer: crate::surfaces::Layer,
    info: &AcceleratedPaintInfo,
) {
    #[cfg(target_os = "macos")]
    {
        let frame = with(|host| {
            let gpu = host.gpu.as_ref()?;
            let p = host.panes.get_mut(&pane)?;
            if layer == crate::surfaces::Layer::Page
                && p.stale_paint(crate::surfaces::size_of(info.shared_texture_io_surface)?)
            {
                return None;
            }
            let ring = p.ring.as_ref()?;
            let slot = p.free.pop();
            let target = slot.map(|slot| (ring, slot));
            let painted = gpu.paint(info.shared_texture_io_surface, layer, &mut p.layers, target);
            match slot {
                Some(slot) if painted => Some((p.generation, slot, p.layers.page_size()?)),
                Some(slot) => {
                    p.free.push(slot);
                    None
                }
                // Kept; shown once the app hands a slot back.
                None => {
                    p.missed = true;
                    None
                }
            }
        })
        .flatten();
        if let Some((generation, slot, size)) = frame {
            send(&HostEvent::Frame {
                pane,
                generation,
                slot,
                size,
            });
        }
    }
}

/// Composes the kept layers into a free slot after the popup moved or
/// closed, or a paint found no free slot.
fn present(pane: PaneId) {
    #[cfg(target_os = "macos")]
    {
        let frame = with(|host| {
            let gpu = host.gpu.as_ref()?;
            let p = host.panes.get_mut(&pane)?;
            let ring = p.ring.as_ref()?;
            if !p.layers.has_page() {
                return None;
            }
            let Some(slot) = p.free.pop() else {
                p.missed = true;
                return None;
            };
            if gpu.recompose(&p.layers, ring, slot) {
                Some((p.generation, slot, p.layers.page_size()?))
            } else {
                p.free.push(slot);
                None
            }
        })
        .flatten();
        if let Some((generation, slot, size)) = frame {
            send(&HostEvent::Frame {
                pane,
                generation,
                slot,
                size,
            });
        }
    }
}

/// A `<select>` list or other popup opened or closed.
pub(crate) fn popup_shown(pane: PaneId, shown: bool) {
    if shown {
        return;
    }
    #[cfg(target_os = "macos")]
    with_pane(pane, |p| p.layers.close_popup());
    present(pane);
}

/// Where the popup is, in the pane's logical coordinates.
pub(crate) fn popup_moved(pane: PaneId, rect: &Rect) {
    #[cfg(target_os = "macos")]
    with_pane(pane, |p| {
        let scale = f64::from(p.viewport.scale);
        let at = |v: i32| (f64::from(v) * scale).round() as i64;
        p.layers.popup_at = Some((at(rect.x), at(rect.y)));
    });
}

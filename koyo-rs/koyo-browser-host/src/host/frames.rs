//! A pane's frames: the ring of shared surfaces it paints into, each GPU paint composed
//! into a free slot and announced, and the slots the app hands back. The app hands a slot
//! back as soon as a newer frame replaces it; a slot it still uses (its GPU still reading it)
//! is marked in use, and isn't painted into until it isn't, looked at again after [`RETRY`].
use std::time::Duration;

use super::{send, with, with_pane};
use cef::{AcceleratedPaintInfo, ImplTask, Rect, Task, ThreadId, WrapTask, rc::Rc, wrap_task};
use koyo_browser_ipc::{self as ipc, HostEvent, PaneId};

/// How soon a frame that found every free slot still in use by the app looks again: a
/// fraction of a 120 Hz display's frame. Each look that finds them in use still doubles it,
/// at most [`BACKOFF`] times, so a window that stops drawing isn't asked hundreds of times a
/// second.
const RETRY: Duration = Duration::from_millis(2);

/// How many times [`RETRY`] doubles at most: to 128 ms.
const BACKOFF: u32 = 6;

/// A free slot of `ring` the app is done with, taken from `free`: the one handed back first,
/// the likeliest to be done with.
#[cfg(target_os = "macos")]
fn take_free(free: &mut Vec<u8>, ring: &crate::surfaces::Ring) -> Option<u8> {
    let at = free.iter().position(|slot| !ring.in_use(*slot))?;
    Some(free.remove(at))
}

/// A pane's latest frame that found no slot free and done with.
#[derive(Default)]
pub(super) struct Behind {
    /// A frame waits for a slot: shown once one is free and done with.
    waiting: bool,
    /// A look at the free slots is due ([`PresentDue`]).
    due: bool,
    /// Looks in a row that found the free slots still in use.
    looks: u32,
}

impl Behind {
    /// A frame was shown: nothing waits any more.
    fn caught_up(&mut self) {
        self.waiting = false;
        self.looks = 0;
    }
}

/// A frame of `pane` missed every slot: kept, and shown once a slot is free and done with.
/// When the app handed slots back it still uses, they're looked at again shortly.
#[cfg(target_os = "macos")]
fn missed(p: &mut super::Pane, pane: PaneId) {
    p.behind.waiting = true;
    if p.free.is_empty() || p.behind.due {
        return;
    }
    p.behind.due = true;
    let wait = RETRY * 2u32.pow(p.behind.looks.min(BACKOFF));
    p.behind.looks += 1;
    let mut due = PresentDue::new(pane);
    cef::post_delayed_task(ThreadId::UI, Some(&mut due), super::panes::delay_ms(wait));
}

wrap_task! {
    struct PresentDue {
        pane: PaneId,
    }

    impl Task {
        fn execute(&self) {
            let waiting = with_pane(self.pane, |p| {
                p.behind.due = false;
                std::mem::take(&mut p.behind.waiting)
            });
            if waiting == Some(true) {
                present(self.pane);
            }
        }
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
            p.behind.caught_up();
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
        p.behind.looks = 0;
        std::mem::take(&mut p.behind.waiting)
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
    with_pane(pane, |p| p.viewport.device_scale()).unwrap_or(1.)
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
            if layer == crate::surfaces::Layer::Page {
                let now = std::time::Instant::now();
                let scaled = super::sizing::scaled(info);
                if p.sizing.drops(scaled, now) {
                    return None;
                }
                let size = crate::surfaces::size_of(info.shared_texture_io_surface)?;
                if !scaled {
                    super::live::drive::painted(p, pane, size);
                }
                if p.sizing.painted(size) {
                    // Laid out once this paint is done with, not from within it: the size
                    // asked for last by then.
                    super::enqueue(super::Queued::SizeNext { pane });
                }
            }
            let ring = p.ring.as_ref()?;
            let slot = take_free(&mut p.free, ring);
            let target = slot.map(|slot| (ring, slot));
            let painted = gpu.paint(info.shared_texture_io_surface, layer, &mut p.layers, target);
            match slot {
                Some(slot) if painted => {
                    p.behind.caught_up();
                    Some((
                        p.generation,
                        slot,
                        super::live::drive::filled(p, p.layers.page_size()?),
                    ))
                }
                Some(slot) => {
                    p.free.push(slot);
                    None
                }
                // Kept; shown once a slot is free and done with.
                None => {
                    missed(p, pane);
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
            let Some(slot) = take_free(&mut p.free, ring) else {
                missed(p, pane);
                return None;
            };
            if gpu.recompose(&p.layers, ring, slot) {
                p.behind.caught_up();
                Some((
                    p.generation,
                    slot,
                    super::live::drive::filled(p, p.layers.page_size()?),
                ))
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

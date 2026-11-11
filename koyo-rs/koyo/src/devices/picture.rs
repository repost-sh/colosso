//! A device's picture on the app's side (`docs/devices.md` 3.1): the ring of the current
//! generation, looked up from the mach ports the host sent once, and the slot shown now as a
//! pixel buffer over it. One slot is held at a time, as the browser holds a page's: the newer is
//! shown, then the one before let go, so the host always has a slot to copy into. An entity of
//! its own, so sixty frames a second redraw the tab and nothing else.

use gpui_kit::Context;
use koyo_browser_ipc::mach;
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;

use crate::browser::frames::{self, Ring};

/// The frame shown: the buffer over its slot, which holds the slot in use.
pub struct Shown {
    pub pixels: CFRetained<CVPixelBuffer>,
}

#[derive(Default)]
pub struct Picture {
    ring: Option<Ring>,
    /// A frame whose ring hasn't come yet: the two travel on different channels.
    waiting: Option<(u32, u8)>,
    pub shown: Option<Shown>,
    /// Frames shown and frames painted, for the stats line and the paint counter's row.
    pub frames: u64,
    pub paints: u64,
}

impl Picture {
    /// A new ring: kept if it is newer than the one held, and a frame that waited for it shown.
    pub fn ring(&mut self, ring: mach::Ring, cx: &mut Context<Self>) {
        if self
            .ring
            .as_ref()
            .is_some_and(|held| held.generation >= ring.generation)
        {
            ring.ports.into_iter().for_each(mach::release);
            return;
        }
        let Some(ring) = frames::import(ring) else {
            log::warn!("a device's pictures can't be drawn: their surfaces didn't come through");
            return;
        };
        let generation = ring.generation;
        self.ring = Some(ring);
        if let Some((waiting, slot)) = self.waiting.take()
            && waiting == generation
        {
            self.show(slot, cx);
        }
    }

    /// A frame is whole in `slot` of the ring of `generation`.
    pub fn frame(&mut self, generation: u32, slot: u8, cx: &mut Context<Self>) {
        match &self.ring {
            Some(ring) if ring.generation == generation => self.show(slot, cx),
            Some(ring) if ring.generation > generation => {}
            _ => self.waiting = Some((generation, slot)),
        }
    }

    fn show(&mut self, slot: u8, cx: &mut Context<Self>) {
        let Some(surface) = self
            .ring
            .as_ref()
            .and_then(|ring| ring.surfaces.get(usize::from(slot)))
        else {
            return;
        };
        let Some(pixels) = frames::pixel_buffer(surface) else {
            return;
        };
        // The newer is held before the one before it is let go.
        self.shown = Some(Shown { pixels });
        self.frames += 1;
        cx.notify();
    }

    /// The host went, or the device changed: nothing of its picture is kept.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.ring = None;
        self.waiting = None;
        self.shown = None;
        cx.notify();
    }
}

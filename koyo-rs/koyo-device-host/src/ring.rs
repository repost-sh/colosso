//! The ring (`docs/devices.md` 3.1): three BGRA `IOSurface`s of the host's own at the picture's
//! upright size, which the app is handed once per generation by mach port. Each frame is copied
//! into a slot that isn't the one last announced and that nobody holds, and announced on the link
//! once the GPU has copied it. With no slot free the frame is owed and copied as soon as one is:
//! the newest frame is never the one lost, since a still simulator sends no other.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use koyo_browser_ipc::mach;
use koyo_device_wire::{FromHost, LogLevel, PixelSize};
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceColorSpace, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};

use crate::gpu::{Gpu, Texture};
use crate::link::Teller;

/// `'BGRA'`, the one format the ring takes and Koyo's renderer draws as it is.
pub const BGRA: u32 = u32::from_be_bytes(*b"BGRA");
const SLOTS: usize = 3;

struct Slot {
    surface: CFRetained<IOSurfaceRef>,
    texture: Texture,
}

/// The frames' source: the surface the device's picture is in, and a texture over it.
struct Source {
    surface: CFRetained<IOSurfaceRef>,
    texture: Texture,
}

#[derive(Default)]
struct Inner {
    generation: u32,
    slots: Vec<Slot>,
    source: Option<Source>,
    /// Quarter turns that put the source's picture upright.
    quarters: u8,
    announced: Option<usize>,
    copying: [bool; SLOTS],
    /// A frame came with no slot free.
    owed: bool,
}

struct Shared {
    gpu: Gpu,
    inner: Mutex<Inner>,
    teller: Teller,
    sender: mach::Sender,
    /// Whether frames are wanted at all: a tab draws the device, or an agent is at work on it.
    wanted: AtomicBool,
    /// Frames copied and announced, ever: what a waiter watches.
    copied: AtomicU64,
    /// Frames the source presented, and those that found no slot free.
    presented: AtomicU64,
    owed: AtomicU64,
}

// SAFETY: every Metal object and surface in `Inner` is used under its lock; the GPU's objects
// are thread-safe (gpu.rs); the mach sender is a port name.
unsafe impl Send for Shared {}
// SAFETY: as above.
unsafe impl Sync for Shared {}

/// A slot held for reading: nobody copies into it until this is dropped.
pub struct Held {
    pub surface: CFRetained<IOSurfaceRef>,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.surface.decrement_use_count();
    }
}

#[derive(Clone)]
pub struct Ring(Arc<Shared>);

/// Counts for the stats line.
pub struct Counts {
    pub presented: u64,
    pub copied: u64,
    pub owed: u64,
}

impl Ring {
    pub fn new(gpu: Gpu, teller: Teller, sender: mach::Sender) -> Ring {
        Ring(Arc::new(Shared {
            gpu,
            inner: Mutex::new(Inner::default()),
            teller,
            sender,
            wanted: AtomicBool::new(false),
            copied: AtomicU64::new(0),
            presented: AtomicU64::new(0),
            owed: AtomicU64::new(0),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The source's surface is `surface` (at the start, or after it was replaced). The same
    /// surface again, under another object, changes nothing: surfaces are told apart by id.
    pub fn mount(&self, surface: CFRetained<IOSurfaceRef>) -> Result<(), String> {
        let mut inner = self.lock();
        if inner
            .source
            .as_ref()
            .is_some_and(|source| source.surface.id() == surface.id())
        {
            return Ok(());
        }
        if surface.pixel_format() != BGRA {
            return Err(format!(
                "the device's picture is in format {:#x}, not BGRA",
                surface.pixel_format()
            ));
        }
        let texture = self
            .0
            .gpu
            .texture(&surface)
            .ok_or("no texture over the device's picture")?;
        inner.source = Some(Source { surface, texture });
        self.remake(&mut inner)?;
        self.copy(&mut inner);
        Ok(())
    }

    /// The picture is to be turned by `quarters` to be upright: a new generation when its
    /// shape changes.
    pub fn turn(&self, quarters: u8) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.quarters == quarters % 4 {
            return Ok(());
        }
        inner.quarters = quarters % 4;
        self.remake(&mut inner)?;
        self.copy(&mut inner);
        Ok(())
    }

    /// A new ring at the upright size, sent to the app.
    fn remake(&self, inner: &mut Inner) -> Result<(), String> {
        let Some(source) = &inner.source else {
            return Ok(());
        };
        let (width, height) = (source.surface.width(), source.surface.height());
        let (width, height) = if inner.quarters % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        };
        let space = source.surface.value(color_space_key());
        let mut slots = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            let surface = surface(width, height).ok_or("no surface for the ring")?;
            if let Some(space) = &space {
                // SAFETY: the source's colour space, as the property it is, onto a surface of
                // the same pixels.
                unsafe { surface.set_value(color_space_key(), space) };
            }
            let texture = self
                .0
                .gpu
                .texture(&surface)
                .ok_or("no texture for the ring")?;
            slots.push(Slot { surface, texture });
        }
        inner.generation += 1;
        inner.slots = slots;
        inner.announced = None;
        inner.copying = [false; SLOTS];
        let ports: Vec<u32> = inner
            .slots
            .iter()
            .map(|slot| slot.surface.create_mach_port())
            .collect();
        // The ports move to the app whether or not the send succeeds.
        if let Err(error) = self.0.sender.send(1, inner.generation, &ports) {
            self.0
                .teller
                .log(LogLevel::Error, format!("the ring wasn't sent: {error}"));
        }
        self.0.teller.tell(&FromHost::Surfaces {
            generation: inner.generation,
            pixels: PixelSize {
                width: u32::try_from(width).unwrap_or(u32::MAX),
                height: u32::try_from(height).unwrap_or(u32::MAX),
            },
        });
        Ok(())
    }

    /// The source presented a frame: copied if it is wanted.
    pub fn frame(&self) {
        self.0.presented.fetch_add(1, Ordering::Relaxed);
        if self.0.wanted.load(Ordering::Relaxed) {
            self.copy(&mut self.lock());
        }
    }

    /// Whether frames are wanted. Wanted again, the picture as it is now is copied at once: a
    /// still screen sends no frame of its own.
    pub fn want(&self, wanted: bool) {
        let was = self.0.wanted.swap(wanted, Ordering::Relaxed);
        if wanted && !was {
            self.copy(&mut self.lock());
        }
    }

    pub fn wanted(&self) -> bool {
        self.0.wanted.load(Ordering::Relaxed)
    }

    /// Copies an owed frame if a slot has come free; whether one is still owed.
    pub fn retry(&self) -> bool {
        let mut inner = self.lock();
        if inner.owed {
            self.copy(&mut inner);
        }
        inner.owed
    }

    fn copy(&self, inner: &mut Inner) {
        let Some(source) = inner.source.as_ref().map(|source| source.texture.clone()) else {
            return;
        };
        let free = (0..inner.slots.len()).find(|&slot| {
            Some(slot) != inner.announced
                && !inner.copying[slot]
                && !inner.slots[slot].surface.is_in_use()
        });
        let Some(slot) = free else {
            if !inner.owed {
                self.0.owed.fetch_add(1, Ordering::Relaxed);
            }
            inner.owed = true;
            return;
        };
        inner.owed = false;
        inner.copying[slot] = true;
        let ring = self.clone();
        let generation = inner.generation;
        let target = inner.slots[slot].texture.clone();
        let copied = self.0.gpu.copy(&source, &target, inner.quarters, move || {
            ring.copied(generation, slot);
        });
        if let Err(error) = copied {
            inner.copying[slot] = false;
            self.0.teller.log(LogLevel::Error, error);
        }
    }

    /// The GPU has copied a frame into `slot`: announced, and an owed one copied.
    fn copied(&self, generation: u32, slot: usize) {
        let mut inner = self.lock();
        if inner.generation != generation {
            return;
        }
        inner.copying[slot] = false;
        inner.announced = Some(slot);
        self.0.copied.fetch_add(1, Ordering::Relaxed);
        self.0.teller.tell(&FromHost::Frame {
            generation,
            slot: u8::try_from(slot).unwrap_or_default(),
        });
        if inner.owed {
            self.copy(&mut inner);
        }
        crate::link::wake();
    }

    /// The newest frame, held until the answer is dropped.
    pub fn latest(&self) -> Option<Held> {
        let inner = self.lock();
        let slot = &inner.slots[inner.announced?];
        slot.surface.increment_use_count();
        Some(Held {
            surface: slot.surface.clone(),
        })
    }

    /// Frames announced, ever.
    pub fn copied_count(&self) -> u64 {
        self.0.copied.load(Ordering::Relaxed)
    }

    /// The upright picture's size in pixels, once there is a source.
    pub fn pixels(&self) -> Option<PixelSize> {
        let inner = self.lock();
        let slot = inner.slots.first()?;
        Some(PixelSize {
            width: u32::try_from(slot.surface.width()).ok()?,
            height: u32::try_from(slot.surface.height()).ok()?,
        })
    }

    pub fn log_error(&self, text: &str) {
        self.0.teller.log(LogLevel::Error, text);
    }

    pub fn counts(&self) -> Counts {
        Counts {
            presented: self.0.presented.load(Ordering::Relaxed),
            copied: self.0.copied.load(Ordering::Relaxed),
            owed: self.0.owed.load(Ordering::Relaxed),
        }
    }
}

fn color_space_key() -> &'static CFString {
    // SAFETY: a constant of the framework, set before main runs.
    unsafe { kIOSurfaceColorSpace }
}

/// A pixel buffer over `surface`, for the decoder to write or Vision to read.
pub fn pixel_buffer(surface: &IOSurfaceRef) -> Option<CFRetained<objc2_core_video::CVPixelBuffer>> {
    let mut buffer: *mut objc2_core_video::CVPixelBuffer = std::ptr::null_mut();
    // SAFETY: a surface that lives for the call; the buffer comes back retained, or not at all.
    let made = unsafe { objc2_core_video::CVPixelBufferCreateWithIOSurface(None, surface, None, std::ptr::NonNull::from(&mut buffer)) };
    let buffer = std::ptr::NonNull::new(buffer).filter(|_| made == 0)?;
    // SAFETY: created above, so this side owns its one retain.
    Some(unsafe { CFRetained::from_raw(buffer) })
}

/// A BGRA surface of `width` by `height` pixels that can be sent to another process.
pub fn surface(width: usize, height: usize) -> Option<CFRetained<IOSurfaceRef>> {
    let number = |value: usize| CFNumber::new_isize(isize::try_from(value).unwrap_or(0));
    let format = CFNumber::new_i64(i64::from(BGRA));
    // SAFETY: the framework's own constants as keys.
    let keys: [&CFString; 4] = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfacePixelFormat,
        ]
    };
    let values: [&CFType; 4] = [&number(width), &number(height), &number(4), &format];
    let properties = CFDictionary::from_slices(&keys, &values);
    // SAFETY: a dictionary of the properties a surface is made with.
    unsafe { IOSurfaceRef::new(properties.as_opaque()) }
}

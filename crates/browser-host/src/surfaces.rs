//! Frame surfaces on macOS: each pane owns a ring of BGRA `IOSurfaces` shared
//! with the app once. CEF's own texture is only valid during
//! `OnAcceleratedPaint`, so every frame is copied on the GPU before the
//! callback returns: into the pane's own copy of its page and of its open
//! popup (a `<select>` list), which are then composed into a free slot.
use colosso_browser_ipc::{PixelSize, RING, mach};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use objc2_metal::{
    MTLBlitCommandEncoder, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLOrigin, MTLPixelFormat, MTLSize, MTLStorageMode,
    MTLTexture, MTLTextureDescriptor, MTLTextureUsage,
};
use std::ffi::c_void;

/// `'BGRA'`.
const BGRA: i32 = 0x4247_5241;

/// The GPU the host copies frames with.
pub(crate) struct Gpu {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
}

/// One shared surface and its texture.
pub(crate) struct Slot {
    surface: CFRetained<IOSurfaceRef>,
    texture: Retained<ProtocolObject<dyn MTLTexture>>,
}

/// A pane's surfaces at one size.
pub(crate) struct Ring {
    pub(crate) size: PixelSize,
    slots: Vec<Slot>,
}

type Texture = Retained<ProtocolObject<dyn MTLTexture>>;

/// What a pane's frames are composed from.
#[derive(Default)]
pub(crate) struct Layers {
    page: Option<Texture>,
    popup: Option<Texture>,
    /// Where the popup sits, in device pixels, while it is open.
    pub(crate) popup_at: Option<(i64, i64)>,
}

impl Layers {
    pub(crate) fn close_popup(&mut self) {
        self.popup = None;
        self.popup_at = None;
    }

    pub(crate) fn has_page(&self) -> bool {
        self.page.is_some()
    }

    /// The size of the page's last paint, which is what a slot shows.
    pub(crate) fn page_size(&self) -> Option<PixelSize> {
        let page = self.page.as_ref()?;
        Some(PixelSize {
            width: u32::try_from(page.width()).ok()?,
            height: u32::try_from(page.height()).ok()?,
        })
    }
}

/// Surfaces are allocated in steps of this many device pixels, so dragging a
/// pane's edge reuses its ring instead of making one for every new size.
const STEP: u32 = 256;

/// The ring size for a page of `size`: room to grow to the next step.
pub(crate) fn capacity(size: PixelSize) -> PixelSize {
    let up = |v: u32| v.max(1).div_ceil(STEP).saturating_mul(STEP);
    PixelSize {
        width: up(size.width),
        height: up(size.height),
    }
}

/// A ring of `ring` holds a page of `size` without wasting much memory: it
/// is large enough, and no larger than the next step up.
pub(crate) fn fits(ring: PixelSize, size: PixelSize) -> bool {
    let room = capacity(size);
    size.width <= ring.width
        && size.height <= ring.height
        && ring.width <= room.width + STEP
        && ring.height <= room.height + STEP
}

/// The size of a paint CEF hands over, if there is one.
pub(crate) fn size_of(frame: *mut c_void) -> Option<PixelSize> {
    let frame = std::ptr::NonNull::new(frame.cast::<IOSurfaceRef>())?;
    // SAFETY: CEF guarantees the IOSurface for the duration of the paint.
    let frame: &IOSurfaceRef = unsafe { frame.as_ref() };
    Some(PixelSize {
        width: u32::try_from(frame.width()).ok()?,
        height: u32::try_from(frame.height()).ok()?,
    })
}

/// The layer a paint is for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    Page,
    Popup,
}

impl Gpu {
    pub(crate) fn new() -> Option<Self> {
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        Some(Self { device, queue })
    }

    fn texture(&self, surface: &IOSurfaceRef) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        // SAFETY: plain 2D descriptor; the size comes from the surface itself.
        let descriptor = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::BGRA8Unorm,
                surface.width(),
                surface.height(),
                false,
            )
        };
        descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
        self.device
            .newTextureWithDescriptor_iosurface_plane(&descriptor, surface, 0)
    }

    /// A new ring of [`RING`] surfaces at `size`.
    pub(crate) fn ring(&self, size: PixelSize) -> Option<Ring> {
        let row = (size.width as usize * 4).next_multiple_of(64);
        let number = |v: i64| CFNumber::new_i64(v);
        let width = number(i64::from(size.width));
        let height = number(i64::from(size.height));
        let element = number(4);
        let bytes_per_row = number(i64::try_from(row).ok()?);
        let format = number(i64::from(BGRA));
        // SAFETY: the keys are IOSurface's own property names.
        let keys: [&CFString; 5] = unsafe {
            [
                kIOSurfaceWidth,
                kIOSurfaceHeight,
                kIOSurfaceBytesPerElement,
                kIOSurfaceBytesPerRow,
                kIOSurfacePixelFormat,
            ]
        };
        let values: [&CFType; 5] = [&width, &height, &element, &bytes_per_row, &format];
        let properties = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
        let slots = (0..RING)
            .map(|_| {
                // SAFETY: a dictionary of the documented creation properties.
                let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }?;
                let texture = self.texture(&surface)?;
                Some(Slot { surface, texture })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Ring { size, slots })
    }

    fn private(&self, width: usize, height: usize) -> Option<Texture> {
        // SAFETY: plain 2D descriptor with a non-zero size.
        let descriptor = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::BGRA8Unorm,
                width.max(1),
                height.max(1),
                false,
            )
        };
        descriptor.setStorageMode(MTLStorageMode::Private);
        descriptor.setUsage(MTLTextureUsage::ShaderRead);
        self.device.newTextureWithDescriptor(&descriptor)
    }

    /// Copies CEF's paint of `layer` into the pane's own copy, then draws
    /// the page and any open popup into `slot` when there is one. Waits for
    /// the GPU, so CEF may reuse its texture once this returns.
    pub(crate) fn paint(
        &self,
        frame: *mut c_void,
        layer: Layer,
        layers: &mut Layers,
        target: Option<(&Ring, u8)>,
    ) -> bool {
        let Some(frame) = std::ptr::NonNull::new(frame.cast::<IOSurfaceRef>()) else {
            return false;
        };
        // SAFETY: CEF guarantees the IOSurface for the duration of the call.
        let frame: &IOSurfaceRef = unsafe { frame.as_ref() };
        let Some(source) = self.texture(frame) else {
            return false;
        };
        let (width, height) = (frame.width(), frame.height());
        let kept = match layer {
            Layer::Page => &mut layers.page,
            Layer::Popup => &mut layers.popup,
        };
        if kept
            .as_ref()
            .is_none_or(|t| t.width() != width || t.height() != height)
        {
            *kept = self.private(width, height);
        }
        let Some(kept) = kept.clone() else {
            return false;
        };
        let Some(commands) = self.queue.commandBuffer() else {
            return false;
        };
        let Some(blit) = commands.blitCommandEncoder() else {
            return false;
        };
        copy(&blit, &source, (0, 0), &kept, (0, 0), (width, height));
        if let Some((ring, slot)) = target {
            compose(&blit, layers, ring, slot);
        }
        blit.endEncoding();
        commands.commit();
        commands.waitUntilCompleted();
        true
    }

    /// Draws the kept page and popup into `slot` again, with no new paint.
    pub(crate) fn recompose(&self, layers: &Layers, ring: &Ring, slot: u8) -> bool {
        if layers.page.is_none() {
            return false;
        }
        let Some(commands) = self.queue.commandBuffer() else {
            return false;
        };
        let Some(blit) = commands.blitCommandEncoder() else {
            return false;
        };
        compose(&blit, layers, ring, slot);
        blit.endEncoding();
        commands.commit();
        commands.waitUntilCompleted();
        true
    }
}

/// The page, then the popup over it, into `slot`.
fn compose(
    blit: &ProtocolObject<dyn MTLBlitCommandEncoder>,
    layers: &Layers,
    ring: &Ring,
    slot: u8,
) {
    let Some(slot) = ring.slots.get(usize::from(slot)) else {
        return;
    };
    let target = (slot.surface.width(), slot.surface.height());
    if let Some(page) = &layers.page {
        copy(blit, page, (0, 0), &slot.texture, (0, 0), target);
    }
    if let (Some(popup), Some((x, y))) = (&layers.popup, layers.popup_at) {
        // Clip to the slot; a popup can hang over the pane's edge.
        let (left, top) = (x.max(0), y.max(0));
        let source = (
            usize::try_from(left - x).unwrap_or(0),
            usize::try_from(top - y).unwrap_or(0),
        );
        let (left, top) = (
            usize::try_from(left).unwrap_or(0),
            usize::try_from(top).unwrap_or(0),
        );
        let width = popup
            .width()
            .saturating_sub(source.0)
            .min(target.0.saturating_sub(left));
        let height = popup
            .height()
            .saturating_sub(source.1)
            .min(target.1.saturating_sub(top));
        copy(
            blit,
            popup,
            source,
            &slot.texture,
            (left, top),
            (width, height),
        );
    }
}

/// Copies the overlap of a `size` region; nothing when it is empty.
fn copy(
    blit: &ProtocolObject<dyn MTLBlitCommandEncoder>,
    from: &ProtocolObject<dyn MTLTexture>,
    at: (usize, usize),
    to: &ProtocolObject<dyn MTLTexture>,
    into: (usize, usize),
    size: (usize, usize),
) {
    let width = size
        .0
        .min(from.width().saturating_sub(at.0))
        .min(to.width().saturating_sub(into.0));
    let height = size
        .1
        .min(from.height().saturating_sub(at.1))
        .min(to.height().saturating_sub(into.1));
    if width == 0 || height == 0 {
        return;
    }
    // SAFETY: both textures are 2D BGRA and the region fits both.
    unsafe {
        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
            from,
            0,
            0,
            MTLOrigin { x: at.0, y: at.1, z: 0 },
            MTLSize { width, height, depth: 1 },
            to,
            0,
            0,
            MTLOrigin { x: into.0, y: into.1, z: 0 },
        );
    }
}

impl Ring {
    /// Send rights to the ring's surfaces, for the app.
    pub(crate) fn ports(&self) -> Vec<u32> {
        self.slots
            .iter()
            .map(|slot| slot.surface.create_mach_port())
            .collect()
    }
}

/// Sends `ring` to the app; the ports move with the message.
pub(crate) fn send(sender: &mach::Sender, pane: u64, generation: u32, ring: &Ring) -> bool {
    sender.send(pane, generation, &ring.ports()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn px(width: u32, height: u32) -> PixelSize {
        PixelSize { width, height }
    }

    #[test]
    fn rings_have_room_to_grow_and_are_replaced_only_past_it() {
        assert_eq!(capacity(px(1000, 600)), px(1024, 768));
        assert_eq!(capacity(px(1024, 1)), px(1024, 256));
        let ring = capacity(px(1000, 600));
        assert!(fits(ring, px(1000, 600)));
        assert!(fits(ring, px(1024, 700)), "growing within the room");
        assert!(!fits(ring, px(1025, 600)), "past the room");
        assert!(fits(ring, px(800, 600)), "a little smaller");
        assert!(!fits(ring, px(500, 300)), "much smaller frees memory");
    }
}

//! Pictures another process paints into surfaces it shares with the app: a ring of them,
//! looked up from the mach ports the host sends once (a device's picture), or one surface a
//! frame (the agent desktop's windows), each drawn by GPUI as a pixel buffer over it; and the
//! tab switcher's own small surfaces, which the renderer draws into and a card reads.

use koyo_browser_ipc::mach;
use objc2_core_foundation::CFRetained;
use objc2_core_video::{CVPixelBuffer, CVPixelBufferCreateWithIOSurface, kCVReturnSuccess};
use objc2_io_surface::IOSurfaceRef;

/// A ring of surfaces for one generation.
pub struct Ring {
    pub generation: u32,
    /// One BGRA surface per slot.
    pub surfaces: Vec<CFRetained<IOSurfaceRef>>,
}

/// Looks the ring's surfaces up and drops the send rights, which the surfaces no longer
/// need. `None` when any of them can't be used.
pub fn import(ring: mach::Ring) -> Option<Ring> {
    let surfaces = ring
        .ports
        .into_iter()
        .map(|port| {
            let surface = IOSurfaceRef::lookup_from_mach_port(port);
            mach::release(port);
            surface
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Ring {
        generation: ring.generation,
        surfaces,
    })
}

/// `surface` as a pixel buffer, which marks it in use for as long as the buffer lives: a host
/// paints only into slots no buffer holds.
pub fn pixel_buffer(surface: &IOSurfaceRef) -> Option<CFRetained<CVPixelBuffer>> {
    let mut buffer = std::ptr::null_mut();
    // SAFETY: `buffer` is a valid out-pointer; no attributes.
    let status = unsafe {
        CVPixelBufferCreateWithIOSurface(None, surface, None, std::ptr::NonNull::from(&mut buffer))
    };
    if status != kCVReturnSuccess {
        log::warn!("a shared surface can't be drawn: CoreVideo said {status}");
        return None;
    }
    // SAFETY: a buffer created for the caller, who owns it.
    std::ptr::NonNull::new(buffer).map(|buffer| unsafe { CFRetained::from_raw(buffer) })
}

/// What `read` makes of the pixels of the surface `buffer` is over, each read as BGRA, of the
/// `width` by `height` at its top left and no more than the surface holds; one past an edge is
/// read at the edge. None where the surface can't be read. For the visual checks.
#[cfg(feature = "visual-check")]
pub fn with_pixels<R>(
    buffer: &CVPixelBuffer,
    width: usize,
    height: usize,
    read: impl FnOnce(&dyn Fn(usize, usize) -> [u8; 4]) -> R,
) -> Option<R> {
    use objc2_core_video::CVPixelBufferGetIOSurface;
    use objc2_io_surface::IOSurfaceLockOptions;
    let surface = CVPixelBufferGetIOSurface(Some(buffer))?;
    let (width, height) = (width.min(surface.width()), height.min(surface.height()));
    if width == 0 || height == 0 {
        return None;
    }
    let row = surface.bytes_per_row();
    // SAFETY: the surface is locked read-only while its bytes are read, and only within its
    // rows and columns, each pixel 4 bytes of BGRA; `buffer` holds it, so no host paints into
    // it meanwhile.
    unsafe {
        if surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) != 0 {
            return None;
        }
        let base = surface.base_address().as_ptr().cast::<u8>();
        let pixel = |x: usize, y: usize| {
            let at = base.add(y.min(height - 1) * row + x.min(width - 1) * 4);
            [*at, *at.add(1), *at.add(2), *at.add(3)]
        };
        let read = read(&pixel);
        surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        Some(read)
    }
}

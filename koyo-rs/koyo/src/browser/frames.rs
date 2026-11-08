//! A pane's frames: the ring of shared surfaces the host paints into, looked up from the
//! mach ports it sends once, each a pixel buffer GPUI's renderer draws in the window's own
//! frame, where the page element lays the page out. Drawn with everything round it, the page
//! is never a frame apart from the card it's in, and the card's rounded corners clip it as
//! they clip the rest of what's in it.

use gpui_kit::{Bounds, ContentMask, Context, DevicePixels, Pixels, WeakEntity, Window};
use koyo_browser_ipc::{PixelSize, mach};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{CVPixelBuffer, CVPixelBufferCreateWithIOSurface, kCVReturnSuccess};
use objc2_io_surface::IOSurfaceRef;

use super::view::BrowserView;

/// A pane's ring of surfaces for one generation.
pub struct Ring {
    pub generation: u32,
    /// One BGRA surface per slot, as GPUI's renderer draws it.
    pub surfaces: Vec<CFRetained<CVPixelBuffer>>,
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
            surface.as_deref().and_then(pixel_buffer)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Ring {
        generation: ring.generation,
        surfaces,
    })
}

/// `surface` as a pixel buffer, which holds on to it.
fn pixel_buffer(surface: &IOSurfaceRef) -> Option<CFRetained<CVPixelBuffer>> {
    let mut buffer = std::ptr::null_mut();
    // SAFETY: `buffer` is a valid out-pointer; no attributes.
    let status = unsafe {
        CVPixelBufferCreateWithIOSurface(None, surface, None, std::ptr::NonNull::from(&mut buffer))
    };
    if status != kCVReturnSuccess {
        log::warn!("a page's shared surface can't be drawn: CoreVideo said {status}");
        return None;
    }
    // SAFETY: a buffer created for the caller, who owns it.
    std::ptr::NonNull::new(buffer).map(|buffer| unsafe { CFRetained::from_raw(buffer) })
}

/// The frame a pane shows: a slot of its ring, and how much of it is page.
#[derive(Clone)]
pub struct Frame {
    pub generation: u32,
    pub slot: u8,
    /// The page's size in device pixels; the surface can be larger.
    pub size: PixelSize,
    /// The device pixels per logical pixel it was painted at.
    pub scale: f32,
    pub pixels: CFRetained<CVPixelBuffer>,
}

impl Frame {
    /// The page's paint in `slot` of a ring, its surface `pixels`, `size` of it the page's:
    /// what the renderer draws of the surface from now on, until the slot holds another.
    pub fn new(
        (generation, slot): (u32, u8),
        size: PixelSize,
        pixels: CFRetained<CVPixelBuffer>,
    ) -> Self {
        gpui_apple::metal_renderer::set_surface_picture(
            &pixels,
            gpui_kit::size(size.width.into(), size.height.into()),
        );
        Frame {
            generation,
            slot,
            size,
            scale: 1.,
            pixels,
        }
    }

    /// Draws the frame with its top left at `bounds`', at `fit` of its true size: in
    /// responsive mode, scaled to fit. Where `bounds` reaches past it, as while a page is laid
    /// out a frame behind its pane growing, the page's edges reach out to fill it.
    pub fn paint(&self, bounds: Bounds<Pixels>, fit: f32, window: &mut Window) {
        let size = gpui_kit::size(
            DevicePixels::from(self.size.width),
            DevicePixels::from(self.size.height),
        );
        let shown = Bounds::new(bounds.origin, size.to_pixels(self.scale / fit));
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_surface(shown, self.pixels.clone());
        });
    }
}

/// A page drawn in a window, kept by the page element for as long as it's drawn: once it
/// isn't, the page is out of sight, and the host stops painting it, unless it's drawn
/// elsewhere since.
pub struct Shown;

impl Shown {
    pub fn new(page: WeakEntity<BrowserView>, cx: &mut Context<Self>) -> Self {
        let me = cx.entity_id();
        cx.on_release(move |_: &mut Self, cx| {
            let _ = page.update(cx, |page, cx| {
                if page
                    .shown
                    .as_ref()
                    .is_some_and(|shown| shown.entity_id() == me)
                {
                    page.shown = None;
                    page.set_visible(false, cx);
                }
            });
        })
        .detach();
        Shown
    }
}

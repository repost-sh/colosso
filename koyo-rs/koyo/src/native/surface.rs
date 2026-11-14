//! A view in a native surface of GPUI's window composition: put where the page lays it out,
//! shown only as far as the page shows it, and taken out of the window with the page.
//!
//! No frame GPUI draws holds such a view, so neither does a picture taken of what the window
//! draws, a tab's for the tab switcher (`shell/pictures.rs`): there a pane can stand for it
//! ([`Surface::mark`]).

use std::cell::{Cell, OnceCell};
use std::ptr::NonNull;

use gpui_apple::metal_renderer::{SurfacePane, set_surface_pane};
use gpui_kit::{
    AnyWindowHandle, App, Bounds, DevicePixels, ElementId, Entity, Pixels,
    PlatformSurfaceAttachment, Size, Window, WindowCompositionSurface,
};
use objc2::Message as _;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

pub struct Surface {
    window: AnyWindowHandle,
    /// The composition's surface the view is in.
    composited: WindowCompositionSurface,
    view: Retained<NSView>,
    /// Where the surface was last put, in the window's size then: it's measured from the
    /// window's bottom. With `visible` and `frame`, so a frame that moves nothing tells
    /// `AppKit` nothing.
    placed: Cell<Option<(Bounds<DevicePixels>, Size<Pixels>)>>,
    /// Whether the surface was last shown.
    visible: Cell<Option<bool>>,
    /// Where the view was last put in its surface.
    frame: Cell<Option<NSRect>>,
    /// What stands for the view in a picture of the window ([`Surface::mark`]): a buffer that
    /// draws nothing, made when first asked for, none if the system gives none; and how a
    /// picture was last told to fill it.
    marker: OnceCell<Option<CFRetained<CVPixelBuffer>>>,
    marked: Cell<Option<SurfacePane>>,
}

impl Surface {
    /// `view` in a native surface of `window`'s composition, taking the pointer over it as
    /// `AppKit` hands it out. Fails where the window can't host a native view, as a test's
    /// can't.
    pub fn new(window: &Window, view: &NSView) -> Result<Surface, String> {
        let composition = (window.enable_window_composition())
            .map_err(|error| format!("no window composition: {error:#}"))?;
        let surface = (composition.create_native_surface())
            .map_err(|error| format!("no native surface: {error:#}"))?;
        let container = (surface.platform_surface())
            .and_then(PlatformSurfaceAttachment::platform_handle)
            .ok()
            .and_then(|handle| handle.downcast::<usize>().ok());
        let Some(container) = container else {
            composition.remove_surface(surface.id()).ok();
            return Err("the native surface isn't an NSView".into());
        };
        // SAFETY: on macOS a native surface's handle is its container `NSView`, which the
        // surface keeps for as long as it lives; `Surface` holds the surface past this borrow.
        let container = unsafe { &*(*container as *const NSView) };
        container.addSubview(view);
        Ok(Surface {
            window: window.window_handle(),
            composited: surface,
            view: view.retain(),
            placed: Cell::new(None),
            visible: Cell::new(None),
            frame: Cell::new(None),
            marker: OnceCell::new(),
            marked: Cell::new(None),
        })
    }

    /// Paints what stands for the view in a picture of the window, over `bounds`: it draws
    /// nothing in the window, and a picture fills it as `pane` says, under what GPUI draws
    /// over the view (`gpui_apple::metal_renderer::set_surface_pane`).
    pub fn mark(&self, bounds: Bounds<Pixels>, pane: SurfacePane, window: &mut Window) {
        let Some(marker) = self.marker.get_or_init(marker) else {
            return;
        };
        if self.marked.replace(Some(pane)) != Some(pane) {
            set_surface_pane(marker, pane);
        }
        window.paint_surface(bounds, marker.clone());
    }

    /// Puts the view over `bounds`, showing only what of it lies within `clip`.
    pub fn place(&self, bounds: Bounds<Pixels>, clip: Bounds<Pixels>, window: &Window) {
        let Ok(surface) = self.composited.platform_surface() else {
            return;
        };
        let visible = bounds.intersect(&clip);
        if visible.is_empty() {
            self.show(surface, false);
            return;
        }
        // The surface is laid out in whole device pixels; the view is offset in it by what it
        // lost to the clip, its own origin at the bottom left.
        let scale = window.scale_factor();
        let device = visible.to_device_pixels(scale);
        let placed = Some((device, window.viewport_size()));
        if self.placed.replace(placed) != placed
            && let Err(error) = surface.set_bounds(device)
        {
            self.placed.set(None);
            log::warn!("couldn't place a native view: {error:#}");
            return;
        }
        self.show(surface, true);
        let from_device = |device: i32| f64::from(device) / f64::from(scale);
        let (left, top) = (
            from_device(device.origin.x.0),
            from_device(device.origin.y.0),
        );
        let height = from_device(device.size.height.0);
        let to_points = |pixels: Pixels| f64::from(f32::from(pixels));
        let (x, y) = (to_points(bounds.left()), to_points(bounds.top()));
        let (width, tall) = (to_points(bounds.size.width), to_points(bounds.size.height));
        let frame = NSRect::new(
            NSPoint::new(x - left, height - (y - top) - tall),
            NSSize::new(width, tall),
        );
        if self.frame.replace(Some(frame)) != Some(frame) {
            self.view.setFrame(frame);
        }
    }

    /// Shows or hides the surface, unless it's so already.
    fn show(&self, surface: &dyn PlatformSurfaceAttachment, visible: bool) {
        if self.visible.replace(Some(visible)) != Some(visible)
            && let Err(error) = surface.set_visible(visible)
        {
            self.visible.set(None);
            log::warn!("couldn't show or hide a native view: {error:#}");
        }
    }

    /// Takes the view out of the window: at once from sight, and its surface once the window
    /// is free to change, the container going with the surface's last handle.
    pub fn remove(self, cx: &mut App) {
        self.view.removeFromSuperview();
        let Surface {
            window,
            composited: surface,
            ..
        } = self;
        cx.defer(move |cx| {
            let removed = window.update(cx, |_, window, _| {
                (window.enable_window_composition())
                    .and_then(|composition| composition.remove_surface(surface.id()))
            });
            // A closed window has taken its views with it.
            if let Ok(Err(error)) = removed {
                log::warn!("couldn't remove a native view's surface: {error:#}");
            }
            drop(surface);
        });
    }
}

/// A buffer to mark as a pane ([`Surface::mark`]): a pixel of it, as none is ever drawn.
fn marker() -> Option<CFRetained<CVPixelBuffer>> {
    let mut buffer = std::ptr::null_mut();
    // SAFETY: `buffer` is a valid out-pointer; the default allocator, no attributes.
    let status = unsafe {
        let made = NonNull::from(&mut buffer);
        CVPixelBufferCreate(None, 1, 1, kCVPixelFormatType_32BGRA, None, made)
    };
    if status != kCVReturnSuccess {
        return None;
    }
    // SAFETY: a buffer created for the caller, who owns it.
    NonNull::new(buffer).map(|buffer| unsafe { CFRetained::from_raw(buffer) })
}

/// What holds a native view in a surface of its own, given up when the view goes.
pub trait Hosted: 'static {
    fn into_surface(self) -> Surface;
}

/// A native view an element keeps under one id for as long as it's drawn: made by `make`
/// the frame it first shows, none where `make` makes none, and its surface taken out of the
/// window the frame it isn't drawn.
pub struct Kept<T>(pub Option<T>);

/// The native view kept under `id`, made by `make` the frame it first shows.
pub fn keep<T: Hosted>(
    id: impl Into<ElementId>,
    window: &mut Window,
    cx: &mut App,
    make: impl FnOnce(&Window) -> Option<T>,
) -> Entity<Kept<T>> {
    window.use_keyed_state(id.into(), cx, |window, cx| {
        cx.on_release(|kept: &mut Kept<T>, cx| {
            if let Some(native) = kept.0.take() {
                native.into_surface().remove(cx);
            }
        })
        .detach();
        Kept(make(window))
    })
}

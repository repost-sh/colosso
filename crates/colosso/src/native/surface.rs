//! A view in a native surface of GPUI's window composition: put where the page lays it out,
//! shown only as far as the page shows it, and taken out of the window with the page.

use std::cell::Cell;

use gpui_kit::{
    AnyWindowHandle, App, Bounds, DevicePixels, DispatchEventResult, Pixels, PlatformInput,
    PlatformSurfaceAttachment, Size, Window, WindowCompositionSurface,
};
use objc2::Message as _;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};

/// Where a windowless surface's pointer goes.
type Pointer = Box<dyn FnMut(&PlatformInput)>;

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
}

impl Surface {
    /// `view` in a native surface of `window`'s composition, taking the pointer over it as
    /// `AppKit` hands it out. Fails where the window can't host a native view, as a test's
    /// can't.
    pub fn new(window: &Window, view: &NSView) -> Result<Surface, String> {
        Self::make(window, view, None)
    }

    /// `view` in a native surface of `window`'s composition, drawn by something outside the
    /// window. Every press, release, move, exit and scroll over it goes to `pointer`, in the
    /// window's logical pixels, a drag that started on it included: `AppKit` would hand them
    /// to the surface's container, which does nothing, and GPUI's elements under it don't
    /// see them, since native content hides what's behind it from GPUI's hit testing.
    pub fn windowless(
        window: &Window,
        view: &NSView,
        pointer: impl FnMut(&PlatformInput) + 'static,
    ) -> Result<Surface, String> {
        Self::make(window, view, Some(Box::new(pointer)))
    }

    fn make(window: &Window, view: &NSView, pointer: Option<Pointer>) -> Result<Surface, String> {
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
        let forwarded = |mut pointer: Pointer| {
            composition.on_input(surface.id(), move |event| {
                pointer(event);
                DispatchEventResult {
                    propagate: false,
                    default_prevented: false,
                }
            })
        };
        if let Some(pointer) = pointer
            && let Err(error) = forwarded(pointer)
        {
            composition.remove_surface(surface.id()).ok();
            return Err(format!("the pointer can't reach a native view: {error:#}"));
        }
        Ok(Surface {
            window: window.window_handle(),
            composited: surface,
            view: view.retain(),
            placed: Cell::new(None),
            visible: Cell::new(None),
            frame: Cell::new(None),
        })
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

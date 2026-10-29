//! The `AppKit` half of the native slider: an `NSSlider` in a native surface of GPUI's window
//! composition, which stacks it over the page and under GPUI's overlays, and the target its
//! moves come through. Everything unsafe the slider needs is here.
//!
//! Adapted from `Glass-HQ/gpui` (Apache-2.0), `crates/gpui_macos/src/native_controls/slider.rs`:
//! the target that hears the slider and its tick marks set up as stops, ported to objc2 and
//! to gpui-fast's native surfaces.

use std::cell::Cell;

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::{
    AnyWindowHandle, App, Bounds, Pixels, PlatformSurfaceAttachment, Window,
    WindowCompositionSurface,
};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{DefinedClass as _, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSSlider, NSTickMarkPosition, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::host::Shown;

define_class!(
    /// Hears the slider move, and says which stop the thumb is at.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ColossoNativeSliderTarget"]
    #[ivars = UnboundedSender<usize>]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(moved:))]
        fn moved(&self, slider: &NSSlider) {
            let at = slider.doubleValue().round().max(0.) as usize;
            // Unheard only once the page holding the slider has gone.
            self.ivars().unbounded_send(at).ok();
        }
    }
);

impl Target {
    fn new(main: MainThreadMarker, tell: UnboundedSender<usize>) -> Retained<Target> {
        let this = Target::alloc(main).set_ivars(tell);
        // SAFETY: `NSObject`'s `init`, on an instance just allocated with its ivars set.
        unsafe { msg_send![super(this), init] }
    }
}

/// An `NSSlider` in the window, over stops numbered from 0 at the left.
pub struct Native {
    window: AnyWindowHandle,
    surface: WindowCompositionSurface,
    slider: Retained<NSSlider>,
    /// Kept for as long as the slider sends to it: `AppKit` doesn't keep a control's target.
    _target: Retained<Target>,
    height: f32,
    /// Where it was last put, so a frame that moves nothing tells `AppKit` nothing.
    frame: Cell<Option<NSRect>>,
}

impl Native {
    /// A slider over `stops` in `window`, telling `tell` which stop the thumb moves to. Fails
    /// off the main thread and where the window can't host a native view.
    pub fn new(
        window: &Window,
        stops: usize,
        tell: UnboundedSender<usize>,
    ) -> Result<Native, String> {
        let main = MainThreadMarker::new().ok_or("not on the main thread")?;
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
        let slider = NSSlider::new(main);
        slider.setMinValue(0.);
        slider.setMaxValue(stops.saturating_sub(1) as f64);
        slider.setNumberOfTickMarks(stops as isize);
        slider.setTickMarkPosition(NSTickMarkPosition::Below);
        slider.setAllowsTickMarkValuesOnly(true);
        slider.setContinuous(true);
        slider.sizeToFit();
        let target = Target::new(main, tell);
        // SAFETY: the target lives as long as the slider is in the window, and is taken off
        // it before either goes (`remove`); `moved:` is the action `Target` defines, taking
        // the slider that sends it.
        unsafe {
            slider.setTarget(Some(&target));
            slider.setAction(Some(sel!(moved:)));
        }
        // SAFETY: on macOS a native surface's handle is its container `NSView`, which the
        // surface keeps for as long as it lives; `Native` holds the surface past this borrow.
        let container = unsafe { &*(*container as *const NSView) };
        container.addSubview(&slider);
        Ok(Native {
            window: window.window_handle(),
            surface,
            height: slider.frame().size.height as f32,
            slider,
            _target: target,
            frame: Cell::new(None),
        })
    }

    /// How tall `AppKit` draws the slider, its tick marks included.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Has the slider show `shown`; it moves the thumb only when the setting has gone to
    /// another stop, which leaves it alone as it reports a move of its own.
    pub fn show(&self, shown: Shown) {
        let slider = &self.slider;
        if slider.doubleValue().round() as usize != shown.at {
            slider.setDoubleValue(shown.at as f64);
        }
        if slider.isEnabled() != shown.enabled {
            slider.setEnabled(shown.enabled);
        }
    }

    /// Puts the slider over `bounds`, showing only what of it lies within `clip`.
    pub fn place(&self, bounds: Bounds<Pixels>, clip: Bounds<Pixels>, window: &Window) {
        let Ok(surface) = self.surface.platform_surface() else {
            return;
        };
        let visible = bounds.intersect(&clip);
        if visible.is_empty() {
            if let Err(error) = surface.set_visible(false) {
                log::warn!("couldn't hide the native slider: {error:#}");
            }
            return;
        }
        // The container is laid out in whole device pixels; the slider is offset in it by
        // what it lost to the clip, its own origin at the bottom left.
        let scale = window.scale_factor();
        let device = visible.to_device_pixels(scale);
        let points = |device: i32| f64::from(device) / f64::from(scale);
        let (left, top) = (points(device.origin.x.0), points(device.origin.y.0));
        let height = points(device.size.height.0);
        let (x, y) = (
            f64::from(f32::from(bounds.left())),
            f64::from(f32::from(bounds.top())),
        );
        let (width, tall) = (
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        );
        let placed = (surface.set_bounds(device)).and_then(|()| surface.set_visible(true));
        if let Err(error) = placed {
            log::warn!("couldn't place the native slider: {error:#}");
            return;
        }
        let frame = NSRect::new(
            NSPoint::new(x - left, height - (y - top) - tall),
            NSSize::new(width, tall),
        );
        if self.frame.replace(Some(frame)) != Some(frame) {
            self.slider.setFrame(frame);
        }
    }

    /// Takes the slider out of the window: at once from sight, and its surface once the
    /// window is free to change, the container going with the surface's last handle.
    pub fn remove(self, cx: &mut App) {
        // SAFETY: a nil target only stops the slider sending; the target is still alive.
        unsafe { self.slider.setTarget(None) };
        self.slider.removeFromSuperview();
        let Native {
            window, surface, ..
        } = self;
        cx.defer(move |cx| {
            let removed = window.update(cx, |_, window, _| {
                (window.enable_window_composition())
                    .and_then(|composition| composition.remove_surface(surface.id()))
            });
            // A closed window has taken its views with it.
            if let Ok(Err(error)) = removed {
                log::warn!("couldn't remove the native slider's surface: {error:#}");
            }
            drop(surface);
        });
    }
}

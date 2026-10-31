//! A pane's frames: the ring of shared surfaces the host paints into, looked up from the
//! mach ports it sends once, and the view one of them shows in: a layer whose contents are
//! the frame's surface, in a native surface of the window's composition, placed where the
//! page element lays it out.

use colosso_browser_ipc::{PixelSize, mach};
use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::{AppContext as _, Bounds, Context, Pixels, PlatformInput, Task, WeakEntity, Window};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CFRetained, CFType, CGPoint, CGRect, CGSize};
use objc2_foundation::NSPoint;
use objc2_io_surface::IOSurfaceRef;
use objc2_quartz_core::{CALayer, kCAGravityTopLeft};

use super::view::BrowserView;
use crate::native::Surface;

/// A pane's ring of surfaces for one generation.
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

/// The frame a pane shows: a slot of its ring, and how much of it is page.
#[derive(Clone)]
pub struct Frame {
    pub generation: u32,
    pub slot: u8,
    /// The page's size in device pixels; the surface can be larger.
    pub size: PixelSize,
    pub surface: CFRetained<IOSurfaceRef>,
    /// Which of the page's paints this is, counting from its first.
    pub serial: u64,
}

define_class!(
    /// The view a page's frames show in. Nothing to `AppKit`'s hit testing, so the pointer
    /// over it reaches GPUI, which forwards it to the page; flipped, so its layer's top left
    /// is the page's.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ColossoPageView"]
    pub struct PageView;

    unsafe impl NSObjectProtocol for PageView {}

    impl PageView {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

/// A page's view in the window, kept by the page element for as long as it's drawn and
/// taken out of the window with it. Without one, in a window that can't host a native view
/// as a test's can't, the page shows nothing.
pub struct Shown {
    native: Option<Native>,
    /// The paint the layer shows, by serial.
    serial: Option<u64>,
    /// The slot it shows, by generation and slot, and the page's size in it.
    frame: Option<(u32, u8, PixelSize)>,
    scale: f64,
    /// Hands the page what the pointer does over its native view, in order.
    _pointer: Option<Task<()>>,
}

struct Native {
    surface: Surface,
    layer: Retained<CALayer>,
}

impl Shown {
    /// The view of `page`, which is told when the view leaves the window.
    pub fn new(page: WeakEntity<BrowserView>, window: &Window, cx: &mut Context<Self>) -> Self {
        let (pointer, mut events) = mpsc::unbounded::<PlatformInput>();
        let native = match Native::new(window, pointer) {
            Ok(native) => Some(native),
            Err(why) => {
                log::warn!("a browser page can't be shown in this window: {why}");
                None
            }
        };
        let handle = window.window_handle();
        let to = page.clone();
        let pointer = native.is_some().then(|| {
            cx.spawn(async move |_, cx| {
                while let Some(event) = events.next().await {
                    let handed = cx.update_window(handle, |_, window, cx| {
                        to.update(cx, |page, cx| page.native_pointer(&event, window, cx))
                    });
                    if !matches!(handed, Ok(Ok(()))) {
                        return;
                    }
                }
            })
        });
        let me = cx.entity_id();
        cx.on_release(move |shown: &mut Self, cx| {
            if let Some(native) = shown.native.take() {
                native.surface.remove(cx);
            }
            // Unless the page shows in a layer drawn since, it's out of sight.
            let _ = page.update(cx, |page, cx| {
                if page
                    .shown
                    .as_ref()
                    .is_some_and(|layer| layer.entity_id() == me)
                {
                    page.shown = None;
                    page.set_visible(false, cx);
                }
            });
        })
        .detach();
        Shown {
            native,
            serial: None,
            frame: None,
            scale: 0.,
            _pointer: pointer,
        }
    }

    /// Whether the pointer over the page goes to its native view, rather than to GPUI's
    /// elements under it.
    pub fn takes_pointer(&self) -> bool {
        self.native.is_some()
    }

    /// Puts the view over `bounds`, showing only what of it lies within `clip`, at the
    /// window's scale.
    pub fn place(&mut self, bounds: Bounds<Pixels>, clip: Bounds<Pixels>, window: &Window) {
        let Some(native) = &self.native else {
            return;
        };
        let scale = f64::from(window.scale_factor());
        if (self.scale - scale).abs() > f64::EPSILON {
            self.scale = scale;
            native.layer.setContentsScale(scale);
        }
        native.surface.place(bounds, clip, window);
    }

    /// Shows `frame`, unless it shows already: its surface becomes the layer's contents,
    /// as far as the page fills it. A slot painted again is set again, by way of nothing, or
    /// Core Animation wouldn't look at it afresh.
    pub fn show(&mut self, frame: &Frame) {
        let Some(native) = &self.native else {
            return;
        };
        if self.serial.replace(frame.serial) == Some(frame.serial) {
            return;
        }
        let shown = (frame.generation, frame.slot, frame.size);
        let previous = self.frame.replace(shown);
        if previous == Some(shown) {
            // SAFETY: nil is always a valid contents value.
            unsafe { native.layer.setContents(None) };
        }
        // A ring's surfaces share one size, so the part of them the page fills changes only
        // with the ring or the page's size.
        let filled = (frame.generation, frame.size);
        if previous.is_none_or(|(generation, _, size)| (generation, size) != filled) {
            let (width, height) = (frame.surface.width(), frame.surface.height());
            let fraction =
                |page: u32, surface: usize| (f64::from(page) / surface.max(1) as f64).clamp(0., 1.);
            native.layer.setContentsRect(CGRect::new(
                CGPoint::new(0., 0.),
                CGSize::new(
                    fraction(frame.size.width, width),
                    fraction(frame.size.height, height),
                ),
            ));
        }
        let cf: &CFType = &frame.surface;
        let contents: &AnyObject = cf.as_ref();
        // SAFETY: an `IOSurface` is one of the object kinds a layer's contents may be.
        unsafe { native.layer.setContents(Some(contents)) };
    }
}

impl Native {
    /// A page view in a native surface of `window`, what the pointer does over it going to
    /// `pointer`.
    fn new(window: &Window, pointer: mpsc::UnboundedSender<PlatformInput>) -> Result<Self, String> {
        let main = MainThreadMarker::new().ok_or("not on the main thread")?;
        // SAFETY: `NSView`'s `init`, on an instance just allocated.
        let view: Retained<PageView> = unsafe { msg_send![PageView::alloc(main), init] };
        view.setWantsLayer(true);
        let layer = view.layer().ok_or("the page view has no layer")?;
        layer.setMasksToBounds(true);
        // The page is painted at the top left of a surface that may be larger.
        // SAFETY: a constant string Core Animation defines.
        layer.setContentsGravity(unsafe { kCAGravityTopLeft });
        let surface = Surface::windowless(window, &view, move |event| {
            let _ = pointer.unbounded_send(event.clone());
        })?;
        Ok(Native { surface, layer })
    }
}

//! The agent's pointer in the page's own view, at the display's rate: the arrow, near black
//! in a white rim over a faint shadow of its outline, and the ring a click sends out, as
//! Core Animation layers. A display link puts them where the pointer's model has them for
//! each frame the display shows, from a copy of the model, whatever GPUI does meanwhile; it
//! pauses while nothing moves. Where there's no display link, before macOS 14, the overlay
//! draws the pointer instead.
//!
//! Everything here is on the main thread, where the page's view lives.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use objc2::Message as _;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{DefinedClass as _, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
use objc2_core_foundation::{CFRetained, CGAffineTransform, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGMutablePath};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{
    CACurrentMediaTime, CADisplayLink, CAFrameRateRange, CALayer, CAShapeLayer, CATransaction,
};

use super::overlay::{BODY, RIM, RING, outline};
use super::{Next, Pointer};
use crate::browser::frames::PageView;

/// The display's full rate, up to 120 Hz where it refreshes that fast.
const SMOOTH: CAFrameRateRange = CAFrameRateRange {
    minimum: 60.,
    maximum: 120.,
    preferred: 120.,
};

define_class!(
    /// What the display link calls for each frame: puts the pointer where its model has it
    /// when that frame shows, and pauses the link once nothing moves.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ColossoPointerTicker"]
    #[ivars = RefCell<Scene>]
    struct Ticker;

    unsafe impl NSObjectProtocol for Ticker {}

    impl Ticker {
        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            let ahead = link.targetTimestamp() - CACurrentMediaTime();
            let shows = Instant::now() + Duration::from_secs_f64(ahead.clamp(0., 0.05));
            let scene = self.ivars().borrow();
            scene.draw(shows);
            if scene.pointer.next(shows) != Next::Frame {
                link.setPaused(true);
            }
        }
    }
);

/// The pointer as the display link draws it.
struct Scene {
    pointer: Pointer,
    /// How much of its true size the page shows at.
    fit: f32,
    /// The arrow, its tip at its origin and pointing right.
    arrow: Retained<CALayer>,
    /// The ring, a round border.
    ring: Retained<CALayer>,
}

impl Scene {
    /// Everything where the model has it at `now`.
    fn draw(&self, now: Instant) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let shot = self.pointer.shot(now);
        self.arrow.setHidden(shot.is_none());
        self.ring
            .setHidden(shot.as_ref().is_none_or(|shot| shot.ring.is_none()));
        if let Some(shot) = shot {
            let fit = f64::from(self.fit);
            let tip = CGPoint::new(f64::from(shot.tip.x) * fit, f64::from(shot.tip.y) * fit);
            let (sin, cos) = f64::from(shot.heading).sin_cos();
            let size = f64::from(shot.size);
            self.arrow.setPosition(tip);
            self.arrow.setAffineTransform(CGAffineTransform {
                a: cos * size,
                b: sin * size,
                c: -sin * size,
                d: cos * size,
                tx: 0.,
                ty: 0.,
            });
            self.arrow.setOpacity(shot.opacity);
            if let Some((radius, shown)) = shot.ring {
                let radius = f64::from(radius);
                let across = radius * 2.;
                self.ring.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(across, across)));
                self.ring.setCornerRadius(radius);
                self.ring.setPosition(tip);
                self.ring.setOpacity(shown * shot.opacity);
            }
        }
        CATransaction::commit();
    }
}

/// The pointer's layers in a page's view, and the display link that moves them.
pub struct Native {
    /// The page's view they're in, and the view over it they're in.
    page: Retained<NSView>,
    marks: Retained<PageView>,
    ticker: Retained<Ticker>,
    link: Retained<CADisplayLink>,
}

impl Native {
    /// The pointer's layers, hidden, over `page`, the page's view; none where the view has
    /// no display link to move them by.
    pub fn new(page: &NSView) -> Option<Native> {
        let main = MainThreadMarker::new()?;
        if !page.respondsToSelector(sel!(displayLinkWithTarget:selector:)) {
            return None;
        }
        // A view nothing hits and flipped, as the page's own is, so its top left is the page's.
        // SAFETY: `NSView`'s `init`, on an instance just allocated.
        let marks: Retained<PageView> = unsafe { msg_send![PageView::alloc(main), init] };
        marks.setFrame(page.bounds());
        marks.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        marks.setWantsLayer(true);
        let ring = CALayer::new();
        ring.setBorderWidth(RING.0.into());
        ring.setHidden(true);
        let arrow = arrow();
        arrow.setHidden(true);
        if let Some(layer) = marks.layer() {
            layer.addSublayer(&ring);
            layer.addSublayer(&arrow);
        }
        page.addSubview(&marks);

        let scene = Scene {
            pointer: Pointer::default(),
            fit: 1.,
            arrow,
            ring,
        };
        let ticker = Ticker::alloc(main).set_ivars(RefCell::new(scene));
        // SAFETY: `NSObject`'s `init`, on an instance just allocated with its ivars set.
        let ticker: Retained<Ticker> = unsafe { msg_send![super(ticker), init] };
        // SAFETY: `tick:` is the method `Ticker` defines, taking the link; the link keeps
        // its target until it's invalidated, which dropping `Native` does.
        let link = unsafe { page.displayLinkWithTarget_selector(&ticker, sel!(tick:)) };
        link.setPreferredFrameRateRange(SMOOTH);
        link.setPaused(true);
        // SAFETY: the main run loop, in the modes a drag or a menu keeps it in too.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        Some(Native {
            page: page.retain(),
            marks,
            ticker,
            link,
        })
    }

    /// Whether they're in `page`.
    pub fn in_view(&self, page: &NSView) -> bool {
        std::ptr::eq(std::ptr::from_ref::<NSView>(&self.page), std::ptr::from_ref(page))
    }

    /// From now on, the pointer as `pointer` has it, the page shown at `fit` of its size and
    /// the ring tinted `tint`.
    pub fn show(&self, pointer: &Pointer, fit: f32, tint: [f64; 3]) {
        let mut scene = self.ticker.ivars().borrow_mut();
        scene.pointer = pointer.clone();
        scene.fit = fit;
        scene.ring.setBorderColor(Some(&color(tint, RING.1.into())));
        drop(scene);
        self.redraw();
    }

    /// The page now shows at `fit` of its size.
    pub fn fit(&self, fit: f32) {
        let mut scene = self.ticker.ivars().borrow_mut();
        if (scene.fit - fit).abs() > f32::EPSILON {
            scene.fit = fit;
            drop(scene);
            self.redraw();
        }
    }

    /// Has it drawn for each frame from the next while it moves, unless it is already.
    pub fn wake(&self) {
        if self.link.isPaused() {
            self.redraw();
        }
    }

    /// Draws it now, and for each frame from the next while it moves.
    fn redraw(&self) {
        let scene = self.ticker.ivars().borrow();
        let now = Instant::now();
        scene.draw(now);
        self.link.setPaused(scene.pointer.next(now) != Next::Frame);
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        self.link.invalidate();
        self.marks.removeFromSuperview();
    }
}

/// The arrow's layers, its tip at their origin and pointing right: a faint shadow of its
/// outline, so the notch stays open, the white rim, and the near-black body.
fn arrow() -> Retained<CALayer> {
    let arrow = CALayer::new();
    let outline = arrow_path();
    let white = color([1., 1., 1.], RIM.1.into());
    let rim = CAShapeLayer::new();
    rim.setPath(Some(&outline));
    rim.setFillColor(Some(&white));
    rim.setStrokeColor(Some(&white));
    rim.setLineWidth(RIM.0.into());
    rim.setShadowColor(Some(&color([0., 0., 0.], 1.)));
    rim.setShadowOpacity(0.28);
    rim.setShadowRadius(1.6);
    rim.setShadowOffset(CGSize::new(0., 0.));
    let body = CAShapeLayer::new();
    body.setPath(Some(&outline));
    body.setFillColor(Some(&color([BODY.into(); 3], 1.)));
    arrow.addSublayer(&rim);
    arrow.addSublayer(&body);
    arrow
}

fn color([r, g, b]: [f64; 3], alpha: f64) -> CFRetained<CGColor> {
    CGColor::new_srgb(r, g, b, alpha)
}

/// The arrow, its tip at the origin and pointing right, every corner rounded.
fn arrow_path() -> CFRetained<CGMutablePath> {
    let corners = outline().map(|corner| corner.map(|(x, y)| (f64::from(x), f64::from(y))));
    let path = CGMutablePath::new();
    let start = corners[0][2];
    // SAFETY: no transform, on a path just made.
    unsafe {
        CGMutablePath::move_to_point(Some(&path), std::ptr::null(), start.0, start.1);
        for [into, at, out] in corners.iter().cycle().skip(1).take(corners.len()) {
            CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), into.0, into.1);
            CGMutablePath::add_quad_curve_to_point(
                Some(&path),
                std::ptr::null(),
                at.0,
                at.1,
                out.0,
                out.1,
            );
        }
    }
    CGMutablePath::close_subpath(Some(&path));
    path
}

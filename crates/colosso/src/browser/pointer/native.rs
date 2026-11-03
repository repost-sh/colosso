//! The agent's marks in the page's own view, at the display's rate: the glow inside the
//! page's edges, the arrow, near black in a white rim over a faint shadow of its outline,
//! and the ring a click sends out, as Core Animation layers. A display link puts them where
//! the models have them for each frame the display shows, from copies of the models,
//! whatever GPUI does meanwhile; it pauses while nothing moves. Where there's no display
//! link, before macOS 14, the overlay draws them instead.
//!
//! Everything here is on the main thread, where the page's view lives.

use std::cell::{Cell, RefCell};
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
    CAAutoresizingMask, CACurrentMediaTime, CADisplayLink, CAFrameRateRange, CALayer,
    CAShapeLayer, CATransaction,
};

use super::overlay::{BODY, GLOW, HAIRLINE, RIM, RING, outline};
use super::{Glow, Next, Pointer};
use crate::browser::frames::PageView;

/// The display's full rate, up to 120 Hz where it refreshes that fast.
const SMOOTH: CAFrameRateRange = CAFrameRateRange {
    minimum: 60.,
    maximum: 120.,
    preferred: 120.,
};

define_class!(
    /// What the display link calls for each frame: puts the marks where their models have
    /// them when that frame shows, and pauses the link once nothing moves.
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
            if !scene.moving(shows) {
                link.setPaused(true);
            }
        }
    }
);

/// The marks as the display link draws them.
struct Scene {
    pointer: Pointer,
    glow: Glow,
    /// The colour of the glow and the ring, as sRGB components.
    tint: Option<[f64; 3]>,
    edge: Edge,
    /// How much of its true size the page shows at.
    fit: f32,
    /// The arrow, its tip at its origin and pointing right.
    arrow: Retained<CALayer>,
    /// The ring, a round border.
    ring: Retained<CALayer>,
}

impl Scene {
    /// Whether anything moves or fades after `now`.
    fn moving(&self, now: Instant) -> bool {
        self.pointer.next(now) == Next::Frame || self.glow.changing(now)
    }

    /// Everything where the models have it at `now`.
    fn draw(&self, now: Instant) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let busy = self.pointer.busy(now);
        self.edge.draw(self.glow.level(now) * (0.8 + 0.2 * busy));
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
        let edge = Edge::new();
        if let Some(layer) = marks.layer() {
            layer.addSublayer(&edge.group);
            layer.addSublayer(&ring);
            layer.addSublayer(&arrow);
        }
        page.addSubview(&marks);

        let scene = Scene {
            pointer: Pointer::default(),
            glow: Glow::default(),
            tint: None,
            edge,
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

    /// From now on, the pointer and the glow as `pointer` and `glow` have them.
    pub fn show(&self, pointer: &Pointer, glow: &Glow) {
        let mut scene = self.ticker.ivars().borrow_mut();
        scene.pointer = pointer.clone();
        scene.glow = glow.clone();
        drop(scene);
        self.redraw();
    }

    /// The page now shows at `fit` of its size, and the marks are tinted `tint`.
    pub fn follow(&self, fit: f32, tint: [f64; 3]) {
        let mut scene = self.ticker.ivars().borrow_mut();
        let tinted = scene.tint.replace(tint) != Some(tint);
        if tinted {
            scene.ring.setBorderColor(Some(&color(tint, RING.1.into())));
            scene.edge.tint(tint);
        }
        if tinted || (scene.fit - fit).abs() > f32::EPSILON {
            scene.fit = fit;
            drop(scene);
            self.redraw();
        }
    }

    /// Has them drawn for each frame from the next while they move, unless they are already.
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
        self.link.setPaused(!scene.moving(now));
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        self.link.invalidate();
        self.marks.removeFromSuperview();
    }
}

/// The glow inside the page's edges: soft inner shadows, each cast onto the page by a frame
/// round it, and a hairline at the edge, all as wide as the page and clipped to it.
struct Edge {
    group: Retained<CALayer>,
    shades: [Retained<CALayer>; 3],
    line: Retained<CALayer>,
    /// The page's size the frame the shades are cast by was last made for.
    size: Cell<CGSize>,
}

impl Edge {
    fn new() -> Edge {
        let fills = CAAutoresizingMask::LayerWidthSizable | CAAutoresizingMask::LayerHeightSizable;
        let group = CALayer::new();
        group.setMasksToBounds(true);
        group.setAutoresizingMask(fills);
        group.setHidden(true);
        let shades = GLOW.map(|(alpha, blur)| {
            let shade = CALayer::new();
            shade.setAutoresizingMask(fills);
            shade.setShadowOffset(CGSize::new(0., 0.));
            // Core Animation's radius reaches as far as GPUI's blur, measured side by side.
            shade.setShadowRadius(f64::from(blur));
            shade.setShadowOpacity(alpha);
            group.addSublayer(&shade);
            shade
        });
        let line = CALayer::new();
        line.setAutoresizingMask(fills);
        line.setBorderWidth(1.);
        group.addSublayer(&line);
        Edge {
            group,
            shades,
            line,
            size: Cell::new(CGSize::new(0., 0.)),
        }
    }

    fn tint(&self, tint: [f64; 3]) {
        for shade in &self.shades {
            shade.setShadowColor(Some(&color(tint, 1.)));
        }
        self.line.setBorderColor(Some(&color(tint, HAIRLINE.into())));
    }

    /// `level` of it shows, from 0 to 1.
    fn draw(&self, level: f32) {
        self.group.setHidden(level <= 0.);
        if level <= 0. {
            return;
        }
        self.group.setOpacity(level);
        let Some(page) = self.group.superlayer().map(|page| page.bounds()) else {
            return;
        };
        let size = page.size;
        if (self.size.get().width, self.size.get().height) != (size.width, size.height) {
            self.size.set(size);
            let bounds = CGRect::new(CGPoint::ZERO, size);
            self.group.setFrame(bounds);
            self.line.setFrame(bounds);
            let frame = frame(size);
            for shade in &self.shades {
                shade.setFrame(bounds);
                shade.setShadowPath(Some(&frame));
            }
        }
    }
}

/// A frame round a page of `size`, wide enough for its shadow to fall only inward: the page
/// is a hole in it, drawn the other way round, so it's left out under any fill rule.
fn frame(size: CGSize) -> CFRetained<CGMutablePath> {
    let (width, height) = (size.width, size.height);
    let reach = 200.;
    let path = CGMutablePath::new();
    let outside = CGRect::new(
        CGPoint::new(-reach, -reach),
        CGSize::new(width + reach * 2., height + reach * 2.),
    );
    // SAFETY: no transform, on a path just made.
    unsafe {
        CGMutablePath::add_rect(Some(&path), std::ptr::null(), outside);
        CGMutablePath::move_to_point(Some(&path), std::ptr::null(), 0., 0.);
        CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), 0., height);
        CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), width, height);
        CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), width, 0.);
    }
    CGMutablePath::close_subpath(Some(&path));
    path
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

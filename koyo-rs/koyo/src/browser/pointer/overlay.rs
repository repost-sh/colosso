//! What an agent's work looks like over its page: a glow inside the page's edges while it
//! holds the page, and its pointer, a navigation arrow riding its arcs, with the ring its
//! clicks send out. They're in the page's own view (`native.rs`), moved and faded at the
//! display's rate whatever the app is doing; drawn here, among the window's overlays, as the
//! page's native layer hides what's under it, only where the page has no native view or no
//! display link. Nothing in them takes the pointer.

use std::time::{Duration, Instant};

use gpui_kit::{
    AnyElement, App, BorderStyle, Bounds, BoxShadow, ContentMask, Context, Corners, Entity, Hsla,
    IntoElement as _, PathBuilder, Pixels, Point, Styled as _, Task, Window, canvas, deferred,
    point, px, quad, size, transparent_black,
};

use objc2::rc::Retained;
use objc2_app_kit::NSView;

use super::native::Native;
use super::{Glow, Next, Pointer, Shot, glow};
use crate::browser::responsive::to_window;
use crate::browser::view::BrowserView;
use crate::gradient;

/// The arrow, its tip at the origin and pointing right: its wings' reach back and out, the
/// notch between them, and how round its tip, wings and notch are.
pub(super) const WING: (f32, f32) = (-21., 10.);
pub(super) const NOTCH: f32 = -16.5;
pub(super) const ROUND: (f32, f32, f32) = (2.6, 2.4, 1.6);
/// The white rim round the arrow: how wide it's stroked, half of it outside the arrow, and
/// how much of it shows.
pub(super) const RIM: (f32, f32) = (4.5, 0.96);
/// How light the arrow's body is.
pub(super) const BODY: f32 = 0.06;
/// The ring a click sends out: how wide it's stroked, and how much of the tint it shows.
pub(super) const RING: (f32, f32) = (1.25, 0.55);
/// The glow inside the page's edges, gone some 30 px in: how much of the tint each of its
/// shadows shows and how blurred it is, GPUI's way, from a soft one to a tight bright one.
pub(super) const GLOW: [(f32, f32); 3] = [(0.6, 16.), (0.42, 6.), (0.5, 2.)];
/// How much of the tint the hairline at the page's edge shows.
pub(super) const HAIRLINE: f32 = 0.45;

/// A page's agent marks: its pointer, its glow, the pointer's layers in the page's view, and
/// the wake-up for the pointer's fade.
pub struct Marks {
    pub pointer: Pointer,
    pub glow: Glow,
    native: Option<Native>,
    wake: Option<(Instant, Task<()>)>,
}

impl Default for Marks {
    fn default() -> Self {
        Marks {
            pointer: Pointer::default(),
            glow: glow(),
            native: None,
            wake: None,
        }
    }
}

impl BrowserView {
    /// Sends the agent's pointer to `(x, y)` of the page, in its CSS pixels, as the page
    /// shows it now: answers how long it takes to get there.
    pub(in crate::browser) fn point_to(
        &mut self,
        x: f64,
        y: f64,
        cx: &mut Context<Self>,
    ) -> Duration {
        let zoom = self.zoom as f32 / 100.;
        let to = point(x as f32 * zoom, y as f32 * zoom);
        let page = self.viewport.map_or(size(0., 0.), |viewport| {
            size(viewport.width as f32, viewport.height as f32)
        });
        let still = cx.reduce_motion();
        let duration = (self.marks.pointer).glide_to(to, page, still, Instant::now());
        self.show_pointer(cx);
        cx.notify();
        duration
    }

    /// The agent's pointer clicked where it is.
    pub(in crate::browser) fn pointer_clicked(&mut self, cx: &mut Context<Self>) {
        self.marks.pointer.click(Instant::now());
        self.show_pointer(cx);
        cx.notify();
    }

    /// Hands the pointer and the glow, as they are now, to their layers in the page's view.
    fn show_pointer(&mut self, cx: &App) {
        let (pointer, glow) = (self.marks.pointer.clone(), self.marks.glow.clone());
        let (fit, tint) = (self.fit(), rgb(tint(cx)));
        if let Some(native) = self.pointer_native(cx) {
            native.follow(fit, tint);
            native.show(&pointer, &glow);
        }
    }

    /// The page's view is laid out: the glow in it is laid for its size (`Native::fit_page`).
    pub(in crate::browser) fn marks_fit_page(&self) {
        if let Some(native) = &self.marks.native {
            native.fit_page();
        }
    }

    /// How much of its true size the page shows at.
    fn fit(&self) -> f32 {
        if self.responsive.fit > 0. {
            self.responsive.fit
        } else {
            1.
        }
    }

    /// The view the page shows in now, if it has one.
    fn page_view(&self, cx: &App) -> Option<Retained<NSView>> {
        (self.shown.as_ref())
            .and_then(gpui_kit::WeakEntity::upgrade)
            .and_then(|shown| shown.read(cx).page_view())
    }

    /// Whether the marks show at `now` and their layers aren't in the view the page shows in
    /// now: the page moved to another, or they were never made.
    fn marks_moved(&self, now: Instant, cx: &App) -> bool {
        let page = self.page_view(cx);
        match (&self.marks.native, page) {
            (Some(native), Some(page)) => !native.in_view(&page),
            (Some(_), None) => true,
            (None, Some(_)) => {
                self.marks.glow.level(now) > 0. || self.marks.pointer.shot(now).is_some()
            }
            (None, None) => false,
        }
    }

    /// The pointer's layers in the view the page shows in now, made there if they're not;
    /// none where the page has no native view.
    fn pointer_native(&mut self, cx: &App) -> Option<&Native> {
        let Some(page) = self.page_view(cx) else {
            self.marks.native = None;
            return None;
        };
        if !(self.marks.native.as_ref()).is_some_and(|native| native.in_view(&page)) {
            self.marks.native = Native::new(&page);
        }
        self.marks.native.as_ref()
    }
}

/// `color` as sRGB components, 0 to 1.
fn rgb(color: Hsla) -> [f64; 3] {
    let color = gpui_kit::Rgba::from(color);
    [f64::from(color.r), f64::from(color.g), f64::from(color.b)]
}

/// The marks over `view`'s page, unless there are none to show; asks for the frames and the
/// wake-up they need to move and fade.
pub(in crate::browser) fn overlay(
    view: &mut BrowserView,
    window: &Window,
    cx: &mut Context<BrowserView>,
) -> Option<AnyElement> {
    let now = Instant::now();
    let glows = view.marks.glow.set(view.control.agent().is_some(), now);
    let dismissed = view.control.person_holds() && view.marks.pointer.dismiss(now);
    if glows || dismissed || view.marks_moved(now, cx) {
        view.show_pointer(cx);
    }
    let (fit, accent) = (view.fit(), tint(cx));
    let marks = &mut view.marks;
    // In the page's view, the marks are their layers' to move and fade.
    if let Some(native) = &marks.native {
        native.follow(fit, rgb(accent));
    }
    let native = marks.native.as_ref();
    let drawn = native.is_none();
    let glow = if drawn { marks.glow.level(now) } else { 0. };
    let busy = marks.pointer.busy(now);
    let shot = drawn.then(|| marks.pointer.shot(now)).flatten();
    let moving = marks.pointer.next(now);
    if moving == Next::Frame || marks.glow.changing(now) {
        match native {
            Some(native) => native.wake(),
            None => window.request_animation_frame(),
        }
    }
    match moving {
        Next::At(at) if marks.wake.as_ref().map(|(when, _)| *when) != Some(at) => {
            let wait = at.saturating_duration_since(now);
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            });
            marks.wake = Some((at, task));
        }
        Next::Frame | Next::At(_) | Next::Never => {}
    }
    if glow <= 0. && shot.is_none() {
        return None;
    }
    let page = cx.entity();
    let marks = canvas(
        |_, _, _| (),
        move |bounds, (), window, cx| {
            let (shown, origin, fit) = shown(&page, bounds, cx);
            if glow > 0. {
                paint_glow(shown, accent, glow * (0.8 + 0.2 * busy), window);
            }
            if let Some(shot) = shot {
                let mask = ContentMask { bounds: shown };
                window.with_content_mask(Some(mask), |window| {
                    let place = |at: Point<f32>| to_window(point(px(at.x), px(at.y)), origin, fit);
                    paint_pointer(&shot, place, accent, window);
                });
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    Some(deferred(marks).into_any_element())
}

/// Where `page` shows in `area`, the element its marks are drawn in, where its top left is,
/// whether it shows or not, and how much of its true size it shows at.
fn shown(
    page: &Entity<BrowserView>,
    area: Bounds<Pixels>,
    cx: &App,
) -> (Bounds<Pixels>, Point<Pixels>, f32) {
    let view = page.read(cx);
    let fit = view.fit();
    let Some(viewport) = view.viewport else {
        return (area, area.origin, fit);
    };
    let laid = Bounds::new(
        view.origin,
        size(
            px(viewport.width as f32 * fit),
            px(viewport.height as f32 * fit),
        ),
    );
    (laid.intersect(&area), view.origin, fit)
}

/// The colour of the marks: the hue of the theme the person set, its first colour, kept
/// light and dark enough to show on a page; a neutral grey with no theme set.
fn tint(cx: &App) -> Hsla {
    let Some(color) = gradient::current(cx).primary() else {
        return gpui_kit::hsla(0., 0., 0.45, 1.);
    };
    // A muted colour keeps its hue, with enough colour in it to show; a grey stays grey.
    let s = if color.s > 0.04 {
        color.s.max(0.6)
    } else {
        color.s
    };
    Hsla {
        s,
        l: color.l.clamp(0.45, 0.62),
        ..color
    }
}

/// The glow inside `bounds`, `level` of it: its shadows, and a hairline at the edge.
fn paint_glow(bounds: Bounds<Pixels>, accent: Hsla, level: f32, window: &mut Window) {
    let shadow = |alpha: f32, blur: f32| BoxShadow {
        color: accent.opacity(alpha * level),
        offset: point(px(0.), px(0.)),
        blur_radius: px(blur),
        spread_radius: px(0.),
        inset: true,
    };
    let shadows = GLOW.map(|(alpha, blur)| shadow(alpha, blur));
    window.paint_inset_shadows(bounds, Corners::default(), &shadows);
    window.paint_quad(quad(
        bounds,
        Corners::default(),
        transparent_black(),
        px(1.),
        accent.opacity(HAIRLINE * level),
        BorderStyle::Solid,
    ));
}

/// The pointer as `shot` shows it, its page points put in the window by `place`. The arrow
/// and its ring keep their size on screen whatever the page's scale.
fn paint_pointer(
    shot: &Shot,
    place: impl Fn(Point<f32>) -> Point<Pixels>,
    accent: Hsla,
    window: &mut Window,
) {
    let opacity = shot.opacity;
    let tip = place(shot.tip);
    if let Some((radius, alpha)) = shot.ring {
        let radius = px(radius);
        let ring = Bounds::new(tip - point(radius, radius), size(radius * 2., radius * 2.));
        window.paint_quad(quad(
            ring,
            Corners::all(radius),
            transparent_black(),
            px(RING.0),
            accent.opacity(RING.1 * alpha * opacity),
            BorderStyle::Solid,
        ));
    }
    paint_arrow(tip, shot.heading, shot.size, opacity, window);
}

/// The navigation arrow, its tip at `tip` heading `heading`, `scale` of its size, as the
/// system's pointer is drawn: one near-black fill, a white rim, and a faint shadow of its
/// outline under it.
fn paint_arrow(tip: Point<Pixels>, heading: f32, scale: f32, opacity: f32, window: &mut Window) {
    let (sin, cos) = heading.sin_cos();
    let at = |(along, side): (f32, f32)| {
        let (along, side) = (along * scale, side * scale);
        tip + point(px(along * cos - side * sin), px(along * sin + side * cos))
    };
    // A shadow of its own outline, so the notch stays open: two faint copies, a little lower.
    for (drop, alpha) in [(1., 0.16), (2.2, 0.08)] {
        let lower = |corner| at(corner) + point(px(0.), px(drop));
        let shadow = rounded(lower, PathBuilder::stroke(px(5.)));
        paint(shadow, Hsla::black().opacity(alpha * opacity), window);
    }
    let rim = rounded(at, PathBuilder::stroke(px(RIM.0)));
    paint(rim, Hsla::white().opacity(RIM.1 * opacity), window);
    let body = rounded(at, PathBuilder::fill());
    paint(body, gpui_kit::hsla(0., 0., BODY, opacity), window);
}

/// The arrow's outline, its tip at the origin and pointing right, every corner rounded off:
/// for each corner, where the edge into it stops, the corner, and where the edge out of it
/// starts, the curve between them bent toward the corner.
pub(super) fn outline() -> [[(f32, f32); 3]; 4] {
    let corners = [
        ((0., 0.), ROUND.0),
        ((WING.0, -WING.1), ROUND.1),
        ((NOTCH, 0.), ROUND.2),
        ((WING.0, WING.1), ROUND.1),
    ];
    let toward = |from: (f32, f32), to: (f32, f32), by: f32| {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let length = dx.hypot(dy).max(f32::EPSILON);
        let by = by.min(length / 2.);
        (from.0 + dx / length * by, from.1 + dy / length * by)
    };
    std::array::from_fn(|i| {
        let (at, radius) = corners[i];
        let n = corners.len();
        let (before, after) = (corners[(i + n - 1) % n].0, corners[(i + 1) % n].0);
        [toward(at, before, radius), at, toward(at, after, radius)]
    })
}

/// The arrow's outline, each of its points put in the window by `place`.
fn rounded(
    place: impl Fn((f32, f32)) -> Point<Pixels>,
    mut path: PathBuilder,
) -> Option<gpui_kit::Path<Pixels>> {
    let corners = outline();
    path.move_to(place(corners[0][2]));
    for [into, at, out] in corners.iter().cycle().skip(1).take(corners.len()) {
        path.line_to(place(*into));
        path.curve_to(place(*out), place(*at));
    }
    path.close();
    path.build().ok()
}

fn paint(path: Option<gpui_kit::Path<Pixels>>, color: Hsla, window: &mut Window) {
    if let Some(path) = path {
        window.paint_path(path, color);
    }
}

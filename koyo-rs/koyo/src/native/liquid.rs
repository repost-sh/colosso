//! Liquid Glass behind an element GPUI draws: an `NSGlassEffectView` in a native surface of
//! the window's composition, which stacks it over the page and under the window's overlays,
//! so the element's content, drawn among them, shows over the glass. It's rounded as the element is, and light or dark as
//! the app's theme is, whatever's behind it, so the theme's text reads on it: left to itself, the
//! glass would go light over a light page and dark over a dark one. Dark, it's shaded in the
//! card's colour, so it's of the theme as the card is. Over a web page, which the theme doesn't
//! colour, it can take the other tone than the page's instead ([`backdrop_toned`],
//! `theme::glass`), shaded deeper so the tone holds, and what's on it is drawn in that tone's
//! palette. In a window that can't host a native view, as a test's can't, or before macOS 26,
//! there's none, and the element keeps a fill of its own.
//!
//! It moves only when the element does: a frame that moves nothing tells `AppKit` nothing.
//!
//! It ends at its own edge. Its shadow would reach past that, but a surface shows only what's over
//! the element, since anything more would hide the page from the pointer: so the shadow would be
//! cut off at the element's rectangle and show only in its corners, as grey squares on a light page.
//! The bright line along that edge is left out as well: the glass is a little larger than what
//! shows of it. In its place GPUI draws a faint one of the theme's, `Palette::glass_edge`, just
//! inside the element's corners, so a glass element doesn't melt into a page as light or as dark
//! as it is.

use std::cell::{Cell, RefCell};
use std::sync::Once;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, Bounds, BoxShadow, ElementId, Hsla, IntoElement as _, ParentElement, Pixels,
    Rgba, Styled, Window, canvas, px,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, NSObjectProtocol as _};
use objc2::{MainThreadMarker, MainThreadOnly as _, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization as _, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSAutoresizingMaskOptions, NSColor, NSGlassEffectView, NSGlassEffectViewStyle, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::kCACornerCurveContinuous;

use super::surface::{self, Hosted, Surface};
use crate::theme::{self, Palette};

/// How dark the glass is on a dark theme: its clear style, which the page shows through, shaded in
/// the card's colour so the theme's text reads on it. Glass as it's made would hide the page.
const DARK_SHADE: f32 = 0.45;
/// How far the glass goes past what shows of it, in points: past the bright line it draws along
/// its own edge, which isn't wanted.
const EDGE: f64 = 2.;

/// How wide the faint line round glass is, in points.
pub const EDGE_WIDTH: f32 = 1.;

/// `surface`, floating over what's under it: with `glass` behind what it holds and a faint line
/// round it, or a raised fill where there's none.
pub fn floating<E: ParentElement + Styled>(surface: E, glass: Option<AnyElement>, p: Palette) -> E {
    match glass {
        Some(glass) => surface.child(glass).shadow(vec![edge(p.glass_edge)]),
        None => surface
            .bg(p.floating())
            .border_1()
            .border_color(p.hairline)
            .shadow_md(),
    }
}

/// A line of `colour` just inside an element's edge, round its own corners: an inset shadow that
/// doesn't blur, which takes no room, so what's on the glass sits where it did without one.
fn edge(colour: Hsla) -> BoxShadow {
    BoxShadow::new(px(0.), px(0.), colour)
        .spread_radius(px(EDGE_WIDTH))
        .inset()
}

/// Glass to put behind the element it's a child of, filling it, its corners `radius` round
/// (at most half its height) and dark or light as the theme is; none where there's no glass.
/// `id` keys it: it's kept as long as it's drawn, made the frame it first shows and taken
/// out of the window the frame it doesn't.
pub fn backdrop(
    id: impl Into<ElementId>,
    radius: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let dark = cx.theme().mode.is_dark();
    let shade = dark.then(|| theme::palette(cx).card.alpha(DARK_SHADE));
    glass(id, radius, Look { dark, shade }, window, cx)
}

/// [`backdrop`], of a tone of its own where `tone` gives one, whatever the theme: dark or not, and
/// shaded in its tint (`theme::glass::look`), for glass over a page, which takes the other tone
/// than the page's. What's on it should be drawn in the palette of that tone.
pub fn backdrop_toned(
    id: impl Into<ElementId>,
    radius: Pixels,
    tone: Option<(bool, Hsla)>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    match tone {
        Some((dark, tint)) => {
            let look = Look {
                dark,
                shade: Some(tint),
            };
            glass(id, radius, look, window, cx)
        }
        None => backdrop(id, radius, window, cx),
    }
}

/// Glass keyed by `id`, its corners `radius` round, looking as `look` says.
fn glass(
    id: impl Into<ElementId>,
    radius: Pixels,
    look: Look,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    #[cfg(feature = "visual-check")]
    if crate::visual::glass_as_fill() {
        return None;
    }
    let pane = surface::keep(id, window, cx, |window| {
        Native::new(window)
            .inspect_err(|why| {
                static SAID: Once = Once::new();
                SAID.call_once(|| log::info!("no Liquid Glass behind the app's bars: {why}"));
            })
            .ok()
    });
    pane.read(cx).0.as_ref()?;
    // Placed as it's painted, not prepainted: GPUI stacks a native surface on the side of
    // its overlays its element is prepainted on (gpui-fast's `begin_base` ends the prepaint
    // that tells), so placed from a deferred element's prepaint the glass would cover what's
    // drawn on it. It starts below the overlays and stays there.
    let glass = canvas(
        |_, _, _| (),
        move |bounds, (), window, cx| {
            let clip = window.content_mask().bounds;
            pane.update(cx, |pane, _| {
                if let Some(native) = &pane.0 {
                    native.place(bounds, clip, radius, look, window);
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    Some(glass.into_any_element())
}

struct Native {
    surface: Surface,
    /// What shows of `view`, which ends past it.
    shown: Retained<NSView>,
    /// As it's made on a light theme. Back from a dark one it's made again, as no style there is
    /// goes back to that: the regular style is a glass of its own, whiter and flatter.
    view: RefCell<Retained<NSGlassEffectView>>,
    /// The corners the glass was last given.
    corners: Cell<Option<f64>>,
    /// How it was last told to look: none until it's placed.
    look: Cell<Option<Look>>,
}

/// How glass looks: as `AppKit` makes it if light, clear if dark, and shaded in `shade` if it has
/// one, as dark glass always does.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    dark: bool,
    shade: Option<Hsla>,
}

/// `colour` as `AppKit` takes it, in sRGB as GPUI draws.
fn ns_colour(colour: Hsla) -> Retained<NSColor> {
    let Rgba { r, g, b, a } = colour.into();
    NSColor::colorWithSRGBRed_green_blue_alpha(r.into(), g.into(), b.into(), a.into())
}

/// Whether the system has Liquid Glass: macOS 26 on.
pub(super) fn has_glass() -> bool {
    AnyClass::get(c"NSGlassEffectView").is_some()
}

/// New glass that takes its look from its own appearance ([`appear`]), not from what's behind
/// it.
pub(super) fn new_glass(main: MainThreadMarker) -> Retained<NSGlassEffectView> {
    let view = NSGlassEffectView::new(main);
    if view.respondsToSelector(sel!(set_adaptiveAppearance:)) {
        // SAFETY: the setter, private, takes an `NSInteger`, of which 1 has the glass
        // follow its view's appearance.
        let _: () = unsafe { msg_send![&view, set_adaptiveAppearance: 1isize] };
    }
    view
}

/// Gives `view` the dark appearance or the light, which its glass is then.
pub(super) fn appear(view: &NSGlassEffectView, dark: bool) {
    // SAFETY: names AppKit defines.
    let name = unsafe {
        if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    view.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
}

/// New glass in `shown`, `EDGE` past it all round whatever size it goes to.
fn glass_in(shown: &NSView) -> Retained<NSGlassEffectView> {
    let view = new_glass(shown.mtm());
    let size = shown.bounds().size;
    view.setFrame(NSRect::new(
        NSPoint::new(-EDGE, -EDGE),
        NSSize::new(size.width + 2. * EDGE, size.height + 2. * EDGE),
    ));
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    shown.addSubview(&view);
    view
}

impl Hosted for Native {
    fn into_surface(self) -> Surface {
        self.surface
    }
}

impl Native {
    fn new(window: &Window) -> Result<Native, String> {
        let main = MainThreadMarker::new().ok_or("not on the main thread")?;
        if !has_glass() {
            return Err("the system has no Liquid Glass".into());
        }
        let shown = NSView::new(main);
        shown.setWantsLayer(true);
        let view = RefCell::new(glass_in(&shown));
        let surface = Surface::new(window, &shown)?;
        Ok(Native {
            surface,
            shown,
            view,
            corners: Cell::new(None),
            look: Cell::new(None),
        })
    }

    /// Puts the glass over `bounds`, as much of it as `clip` shows, looking as `look` says.
    fn place(
        &self,
        bounds: Bounds<Pixels>,
        clip: Bounds<Pixels>,
        radius: Pixels,
        look: Look,
        window: &Window,
    ) {
        let was = self.look.replace(Some(look));
        let (was_dark, dark) = (was.map(|was| was.dark), look.dark);
        if was_dark != Some(dark) {
            if was_dark == Some(true) {
                self.view
                    .replace(glass_in(&self.shown))
                    .removeFromSuperview();
                self.corners.set(None);
            }
            let view = self.view.borrow();
            appear(&view, dark);
            if dark {
                view.setStyle(NSGlassEffectViewStyle::Clear);
            }
        }
        // Again as the theme's colour changes, which leaves the appearance as it was.
        if was != Some(look) {
            (self.view.borrow()).setTintColor(look.shade.map(ns_colour).as_deref());
        }
        let corners = f64::from(f32::from(radius.min(bounds.size.height / 2.)));
        if self.corners.replace(Some(corners)) != Some(corners) {
            self.view.borrow().setCornerRadius(corners + EDGE);
            self.end_at(corners);
        }
        self.surface.place(bounds, clip, window);
    }

    /// Shows nothing of the glass outside corners this round, which go as its own do.
    fn end_at(&self, corners: f64) {
        if let Some(layer) = self.shown.layer() {
            layer.setCornerRadius(corners);
            // SAFETY: a name Core Animation defines.
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
        }
    }
}

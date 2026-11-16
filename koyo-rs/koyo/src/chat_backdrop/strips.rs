//! What lies over a chat's picture: the blur and the dots. GPUI can't fade an image with a
//! gradient, but it lays an element's opacity over the images in it: so the blur is a stack of
//! thin strips across the lower part of the pane, each a little more opaque than the one above
//! it, from nothing where the blur starts to whole at the bottom, and the dots lie over it in
//! strips that fade out as the blur's fade in, so the bottom is a plain blur. The blur is smooth,
//! so the strips' steps don't show. Each strip draws its images whole, as big as the screen or a
//! tile, clipped to it: the strips cut no image, so none moves by a part of a pixel.
//!
//! Order, bottom up: the picture, the blur's strips, the dots' strips, then the chat.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{
    AnyElement, App, Bounds, ContentMask, Corners, Div, IntoElement as _, ParentElement as _,
    Pixels, RenderImage, Size, Styled as _, Window, canvas, div, point, relative,
};

use super::dots::{self, Grid};
use super::element::Picture;
use crate::settings::ChatBackdrop;
use crate::viewer::bitmap;

/// How many strips the blur fades in over, and the dots fade out over.
const STRIPS: usize = 32;
/// How many dot tiles are kept, each for a dot size, a density and a screen's scale.
const KEPT_TILES: usize = 4;

/// Where a chat's picture was painted this frame, for the strips over it.
pub struct Placed {
    /// Where the screen lies relative to the window, and what of the pane is on it.
    pub screen: Bounds<Pixels>,
    pub shown: Bounds<Pixels>,
    /// The screen's blur, once it's made; and the dots' tile, with its size in points.
    pub blur: Option<Arc<RenderImage>>,
    pub dots: (Arc<RenderImage>, Size<Pixels>),
}

/// What a chat's picture says of where it was, for the strips painted after it; none while
/// it has nothing to show, and the card shows.
pub type Shared = Rc<RefCell<Option<Placed>>>;

/// A chat's backdrop, looking as `settings` say: the picture, the blur's strips, the dots'.
pub fn backdrop(settings: ChatBackdrop) -> AnyElement {
    let placed = Shared::default();
    let start = if settings.blur {
        settings.blur_start.clamp(0., 1.)
    } else {
        1.
    };
    let strips = fading(start);
    let blur = (settings.blur).then(|| {
        (strips.iter()).map(|&(top, height, blurred)| {
            layer(top, height, blurred, paint_blur, &placed)
        })
    });
    let faded = (settings.blur).then(|| {
        (strips.iter()).map(|&(top, height, blurred)| {
            layer(top, height, 1. - blurred, paint_dots, &placed)
        })
    });
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(Picture {
            settings,
            placed: placed.clone(),
        })
        .children(blur.into_iter().flatten())
        .child(layer(0., start, 1., paint_dots, &placed))
        .children(faded.into_iter().flatten())
        .into_any_element()
}

/// The strips the blur fades in over from `start`, a part of the pane's height, to its bottom:
/// each one's top, how tall it is, both as parts of the pane's height, and how much of the blur
/// shows in it, the last one whole.
fn fading(start: f32) -> Vec<(f32, f32, f32)> {
    let step = (1. - start) / STRIPS as f32;
    (0..STRIPS)
        .map(|index| {
            let blurred = (index + 1) as f32 / STRIPS as f32;
            (start + index as f32 * step, step, blurred)
        })
        .collect()
}

/// A strip across the pane from `top` down, `height` tall, both as parts of the pane's height,
/// in which `paint` draws over what `placed` says at `opacity`.
fn layer(
    top: f32,
    height: f32,
    opacity: f32,
    paint: fn(Bounds<Pixels>, &Placed, &mut Window),
    placed: &Shared,
) -> Div {
    let placed = placed.clone();
    div()
        .absolute()
        .left_0()
        .right_0()
        .top(relative(top))
        .h(relative(height))
        .opacity(opacity)
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    if let Some(placed) = placed.borrow().as_ref() {
                        paint(bounds, placed, window);
                    }
                },
            )
            .size_full(),
        )
}

/// The blur over `strip`: the whole of it, as big as the screen, clipped to the strip.
fn paint_blur(strip: Bounds<Pixels>, placed: &Placed, window: &mut Window) {
    let Some(blur) = &placed.blur else {
        return;
    };
    let clip = strip.intersect(&placed.shown);
    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
        let screen = placed.screen;
        window
            .paint_image(screen, screen, Corners::default(), blur.clone(), 0, false)
            .ok();
    });
}

/// The dots over `strip`: whole tiles from the screen's top left, clipped to the strip, so the
/// grid holds still as the window moves and is one grid across panes.
fn paint_dots(strip: Bounds<Pixels>, placed: &Placed, window: &mut Window) {
    let clip = strip.intersect(&placed.shown);
    if clip.is_empty() {
        return;
    }
    let (tile, side) = &placed.dots;
    let origin = placed.screen.origin;
    let first = |from: Pixels, origin: Pixels, step: Pixels| {
        origin + step * ((from - origin) / step).floor()
    };
    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
        let mut y = first(clip.top(), origin.y, side.height);
        while y < clip.bottom() {
            let mut x = first(clip.left(), origin.x, side.width);
            while x < clip.right() {
                let bounds = Bounds::new(point(x, y), *side);
                window
                    .paint_image(bounds, bounds, Corners::default(), tile.clone(), 0, false)
                    .ok();
                x += side.width;
            }
            y += side.height;
        }
    });
}

thread_local! {
    /// The dot tiles made last, each with its grid.
    static TILES: RefCell<Vec<(Grid, Arc<RenderImage>)>> = const { RefCell::new(Vec::new()) };
}

/// The dot tile of `grid`, made the first time it's asked for; the oldest let go past
/// [`KEPT_TILES`].
pub fn tile(grid: Grid, cx: &mut App) -> Arc<RenderImage> {
    let (tile, old) = TILES.with_borrow_mut(|tiles| {
        if let Some((_, tile)) = tiles.iter().find(|(made, _)| *made == grid) {
            return (tile.clone(), None);
        }
        let tile = dots::tile(grid);
        tiles.push((grid, tile.clone()));
        let old = (tiles.len() > KEPT_TILES).then(|| tiles.remove(0).1);
        (tile, old)
    });
    bitmap::release(old.into_iter().collect(), cx);
    tile
}

#[cfg(test)]
mod tests {
    use super::{STRIPS, fading};

    #[test]
    fn the_blur_fades_in_over_strips_that_meet_from_where_it_starts_to_the_bottom() {
        let strips = fading(0.5);
        assert_eq!(strips.len(), STRIPS);
        assert!((strips[0].0 - 0.5).abs() < 1e-6, "from where it starts");
        for pair in strips.windows(2) {
            let ((top, height, blurred), (next, _, more)) = (pair[0], pair[1]);
            assert!((top + height - next).abs() < 1e-6, "no gap, no overlap");
            assert!(more > blurred, "a little more of it each strip");
        }
        let (top, height, blurred) = strips[STRIPS - 1];
        assert!((top + height - 1.).abs() < 1e-6, "to the bottom");
        assert!((blurred - 1.).abs() < 1e-6, "where it's all there is");
    }
}

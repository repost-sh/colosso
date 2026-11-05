//! Where a viewer's pages are: laid one under the other at a zoom, padded round and spaced
//! apart by amounts that don't zoom, centred while they're smaller than the pane and scrolled
//! once they're larger. Zooming keeps the point under the pointer (or the fingers) where it
//! is, as Preview does. Lengths are the window's logical pixels; a page's size is in points,
//! which a zoom of 1 shows as as many logical pixels.

/// Room round the pages, at any zoom.
pub const PAD: f32 = 16.;
/// Room between pages, at any zoom.
pub const GAP: f32 = 12.;
/// How far out and in a viewer zooms.
pub const LEAST_ZOOM: f32 = 0.02;
pub const MOST_ZOOM: f32 = 64.;
/// The zooms ⌘= and ⌘- step through, as a browser's do.
const STEPS: [f32; 25] = [
    0.05, 0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 4., 5.,
    6., 8., 12., 16., 24., 32., 64.,
];

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Where it overlaps `other`, if it does.
    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then_some(Rect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }

    /// Moved by `dx`, `dy`.
    pub fn shifted(self, (dx, dy): (f32, f32)) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// How a viewer's zoom fits its pages to the pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// The whole page in sight, made no larger than `most` when there's a most: a picture
    /// isn't blown up past its own size, as its pixels would only grow.
    Whole { most: Option<f32> },
    /// As wide as the pane, as a document's pages are read.
    Width,
}

/// How a viewer looks at its pages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub zoom: f32,
    /// How far the pages are scrolled, right and down.
    pub scroll: (f32, f32),
    /// Whether the zoom follows the pane's size, as it does until it's zoomed.
    pub fit: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            zoom: 1.,
            scroll: (0., 0.),
            fit: true,
        }
    }
}

/// The pages at a zoom: each one's place, and how big they are together with the room round
/// them.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub pages: Vec<Rect>,
    pub size: (f32, f32),
}

/// `pages`, sized in points, laid out at `zoom`.
pub fn layout(pages: &[(f32, f32)], zoom: f32) -> Layout {
    let widest = pages.iter().map(|(width, _)| width * zoom).fold(0., f32::max);
    let mut y = PAD;
    let mut rects = Vec::with_capacity(pages.len());
    for (width, height) in pages {
        let (width, height) = (width * zoom, height * zoom);
        rects.push(Rect {
            x: PAD + (widest - width) / 2.,
            y,
            width,
            height,
        });
        y += height + GAP;
    }
    let height = if pages.is_empty() { 2. * PAD } else { y - GAP + PAD };
    Layout {
        pages: rects,
        size: (widest + 2. * PAD, height),
    }
}

/// The zoom that fits `pages` to a pane of `viewport` as `fit` says.
pub fn fit_zoom(pages: &[(f32, f32)], viewport: (f32, f32), fit: Fit) -> f32 {
    let widest = pages.iter().map(|(width, _)| *width).fold(0., f32::max);
    let tallest = pages.iter().map(|(_, height)| *height).fold(0., f32::max);
    if widest <= 0. || tallest <= 0. {
        return 1.;
    }
    let across = (viewport.0 - 2. * PAD).max(1.) / widest;
    let zoom = match fit {
        Fit::Width => across,
        Fit::Whole { most } => {
            let down = (viewport.1 - 2. * PAD).max(1.) / tallest;
            let whole = across.min(down);
            most.map_or(whole, |most| whole.min(most))
        }
    };
    zoom.clamp(LEAST_ZOOM, MOST_ZOOM)
}

/// Where the pages' top left corner goes in the pane, scrolled by `scroll`: centred along
/// either axis they're smaller than the pane along.
pub fn offset(size: (f32, f32), viewport: (f32, f32), scroll: (f32, f32)) -> (f32, f32) {
    let along = |size: f32, viewport: f32, scroll: f32| {
        if size <= viewport {
            (viewport - size) / 2.
        } else {
            -scroll
        }
    };
    (
        along(size.0, viewport.0, scroll.0),
        along(size.1, viewport.1, scroll.1),
    )
}

/// `scroll` kept to what pages of `size` in a pane of `viewport` can be scrolled by.
pub fn clamp(scroll: (f32, f32), size: (f32, f32), viewport: (f32, f32)) -> (f32, f32) {
    (
        scroll.0.clamp(0., (size.0 - viewport.0).max(0.)),
        scroll.1.clamp(0., (size.1 - viewport.1).max(0.)),
    )
}

/// `camera` zoomed to `zoom`, the point of the pages at `anchor` in the pane staying there
/// where the pages are larger than the pane.
pub fn zoom_about(
    pages: &[(f32, f32)],
    viewport: (f32, f32),
    camera: Camera,
    zoom: f32,
    anchor: (f32, f32),
) -> Camera {
    let zoom = zoom.clamp(LEAST_ZOOM, MOST_ZOOM);
    let before = layout(pages, camera.zoom);
    let (left, top) = offset(before.size, viewport, camera.scroll);
    let at = (anchor.0 - left, anchor.1 - top);
    let after = layout(pages, zoom);
    // The point is held by the page it's on, or the nearest, so the room between pages,
    // which doesn't zoom, doesn't move it.
    let target = match nearest(&before.pages, at.1) {
        Some(index) => {
            let (was, now) = (before.pages[index], after.pages[index]);
            let local = ((at.0 - was.x) / camera.zoom, (at.1 - was.y) / camera.zoom);
            (now.x + local.0 * zoom, now.y + local.1 * zoom)
        }
        None => at,
    };
    let scroll = (target.0 - anchor.0, target.1 - anchor.1);
    Camera {
        zoom,
        scroll: clamp(scroll, after.size, viewport),
        fit: false,
    }
}

/// The page whose rows `y` is among, or the nearest one to it.
fn nearest(pages: &[Rect], y: f32) -> Option<usize> {
    (0..pages.len()).min_by(|a, b| {
        let distance = |page: Rect| {
            if y < page.y {
                page.y - y
            } else if y > page.bottom() {
                y - page.bottom()
            } else {
                0.
            }
        };
        distance(pages[*a]).total_cmp(&distance(pages[*b]))
    })
}

/// The next zoom of the steps ⌘= goes through, in or out from `zoom`.
pub fn step(zoom: f32, inward: bool) -> f32 {
    let next = if inward {
        STEPS.iter().find(|step| **step > zoom * 1.01)
    } else {
        STEPS.iter().rev().find(|step| **step < zoom / 1.01)
    };
    next.copied().unwrap_or(zoom).clamp(LEAST_ZOOM, MOST_ZOOM)
}

/// The page most of the pane shows, by the middle of the pane: the page "Page 3 of 12" counts.
pub fn page_in_sight(layout: &Layout, viewport: (f32, f32), scroll: (f32, f32)) -> usize {
    let (_, top) = offset(layout.size, viewport, scroll);
    nearest(&layout.pages, viewport.1 / 2. - top).unwrap_or(0)
}

#[cfg(test)]
mod tests;

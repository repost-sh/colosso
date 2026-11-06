//! A page drawn in tiles: squares of the screen's pixels at one scale, drawn only where the
//! page is in sight, so a page zoomed far in costs the screen it fills rather than the page
//! it would be whole. Each tile is drawn at the zoom it's shown at, pixel for pixel; while a
//! zoom changes, the tiles drawn before are stretched to it until the new ones come.

use super::canvas::{Layout, Rect};

/// A tile's side, in the screen's pixels: a megabyte of them.
pub const TILE: u32 = 512;

/// The scale tiles are drawn at: the screen's pixels a point, to a thousandth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Level(u32);

impl Level {
    pub fn of(scale: f32) -> Level {
        Level((scale * 1000.).round().max(1.) as u32)
    }

    pub fn scale(self) -> f32 {
        self.0 as f32 / 1000.
    }
}

/// One tile: of which page, at what scale, and which column and row of the page's tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub page: usize,
    pub level: Level,
    pub col: u32,
    pub row: u32,
}

/// A page of `points` drawn whole at `level`, in the screen's pixels.
pub fn page_pixels(points: (f32, f32), level: Level) -> (u32, u32) {
    let scale = level.scale();
    (
        (points.0 * scale).ceil().max(1.) as u32,
        (points.1 * scale).ceil().max(1.) as u32,
    )
}

impl TileKey {
    /// The pixels of its page it covers, from the page's top left: left, top, width and
    /// height. A tile on the page's right or bottom edge is cut to the page.
    pub fn pixels(self, points: (f32, f32)) -> (u32, u32, u32, u32) {
        let (width, height) = page_pixels(points, self.level);
        let (left, top) = (self.col * TILE, self.row * TILE);
        (
            left,
            top,
            TILE.min(width.saturating_sub(left)),
            TILE.min(height.saturating_sub(top)),
        )
    }

    /// Where it's shown, on a page shown at `page` at `zoom`, whatever scale it was drawn at.
    pub fn shown_at(self, points: (f32, f32), page: Rect, zoom: f32) -> Rect {
        let (left, top, width, height) = self.pixels(points);
        let per_pixel = zoom / self.level.scale();
        Rect {
            x: page.x + left as f32 * per_pixel,
            y: page.y + top as f32 * per_pixel,
            width: width as f32 * per_pixel,
            height: height as f32 * per_pixel,
        }
    }
}

/// The tiles at `level` of the pages `layout` lays out at `zoom` that `sight`, a rectangle of
/// the laid-out pages, takes in: those nearest its middle first.
pub fn wanted(
    pages: &[(f32, f32)],
    layout: &Layout,
    sight: Rect,
    zoom: f32,
    level: Level,
) -> Vec<TileKey> {
    let middle = (sight.x + sight.width / 2., sight.y + sight.height / 2.);
    let pixels_per = level.scale() / zoom;
    let mut tiles = Vec::new();
    for (page, (points, rect)) in pages.iter().zip(&layout.pages).enumerate() {
        let Some(seen) = rect.intersect(sight) else {
            continue;
        };
        let (width, height) = page_pixels(*points, level);
        let (cols, rows) = (width.div_ceil(TILE), height.div_ceil(TILE));
        let span = |from: f32, to: f32, count: u32| {
            let first = (from * pixels_per / TILE as f32).floor().max(0.) as u32;
            let last = ((to * pixels_per / TILE as f32).ceil() as u32).min(count);
            first..last
        };
        for row in span(seen.y - rect.y, seen.bottom() - rect.y, rows) {
            for col in span(seen.x - rect.x, seen.right() - rect.x, cols) {
                tiles.push(TileKey {
                    page,
                    level,
                    col,
                    row,
                });
            }
        }
    }
    let distance = |key: &TileKey| {
        let shown = key.shown_at(pages[key.page], layout.pages[key.page], zoom);
        let (x, y) = (shown.x + shown.width / 2., shown.y + shown.height / 2.);
        (x - middle.0).powi(2) + (y - middle.1).powi(2)
    };
    tiles.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    tiles
}

#[cfg(test)]
mod tests {
    use super::super::canvas::{Rect, layout};
    use super::{Level, TILE, TileKey, wanted};

    #[test]
    fn a_tile_on_the_pages_edge_is_cut_to_it() {
        let key = TileKey {
            page: 0,
            level: Level::of(2.),
            col: 1,
            row: 0,
        };
        // 300 points at 2 pixels a point: 600 pixels, the second tile 88 of them.
        assert_eq!(key.pixels((300., 100.)), (TILE, 0, 600 - TILE, 200));
    }

    #[test]
    fn only_the_tiles_in_sight_are_wanted_middle_first() {
        let pages = [(1000., 1000.)];
        let laid = layout(&pages, 1.);
        let page = laid.pages[0];
        // A 300 by 200 window onto the page's middle, at 2 pixels a point.
        let sight = Rect {
            x: page.x + 400.,
            y: page.y + 400.,
            width: 300.,
            height: 200.,
        };
        let tiles = wanted(&pages, &laid, sight, 1., Level::of(2.));
        let mut cells: Vec<_> = tiles.iter().map(|key| (key.col, key.row)).collect();
        // The sight's middle, pixel (1100, 1000), is in the third column's second row.
        assert_eq!(tiles.first().map(|key| (key.col, key.row)), Some((2, 1)));
        cells.sort_unstable();
        // Pixels 800 to 1400 across and 800 to 1200 down.
        assert_eq!(cells, [(1, 1), (1, 2), (2, 1), (2, 2)]);
    }

    #[test]
    fn a_tile_drawn_at_another_scale_is_stretched_to_the_zoom() {
        let key = TileKey {
            page: 0,
            level: Level::of(1.),
            col: 1,
            row: 1,
        };
        let page = Rect {
            x: 10.,
            y: 20.,
            width: 2000.,
            height: 2000.,
        };
        let shown = key.shown_at((1000., 1000.), page, 2.);
        assert_eq!(
            (shown.x, shown.y, shown.width),
            (10. + 1024., 20. + 1024., 976.)
        );
    }
}

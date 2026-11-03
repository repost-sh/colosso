//! A pinned grid laid out in the bounds its pane gives it (`docs/responsive.md` 3.2): the font
//! size it's drawn at, fitted as agent-terminal's dashboard fits its font (`fitFont`) or a
//! share of the person's, and where the grid sits. Pure, so its arithmetic is tested without
//! a window. What's round the grid (the bar, the sunken area, the outline and the grips) is
//! the app's.

use gpui_kit::{Bounds, Pixels, Point, point, px, size};

use crate::glyphs::Metrics;

/// The font size moves in these steps.
pub const STEP: f32 = 0.25;
/// The smallest font size a pinned grid is drawn at; a grid that's too large even at this
/// size runs past its bounds, and the pane clips it.
pub const FLOOR: f32 = 4.;

/// How large a pinned grid's font is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// As large as the bounds hold, at most the person's (`Fitting`).
    Fit,
    /// A percent of the person's, whatever the bounds.
    Percent(u32),
    /// As last fitted: a grip in hand keeps the font it was taken at, so the grid's edge
    /// follows the pointer.
    Held,
}

/// `percent` of the `person`'s font size, in whole steps and never under the floor.
pub fn font_at(percent: u32, person: f32) -> f32 {
    let font = person * percent as f32 / 100.;
    ((font / STEP).round() * STEP).max(FLOOR)
}

/// What a font size was fitted to: the room the grid has, its cells, and the person's own
/// font size, which the fitted one never exceeds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub room: (f32, f32),
    pub cells: (u16, u16),
    pub person: f32,
}

/// The font size a pinned grid is drawn at, and what keeps it from flickering: a size that
/// overflowed once for a layout isn't tried again until the layout changes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fitting {
    layout: Option<Layout>,
    overflows_from: Option<f32>,
    font: Option<f32>,
}

impl Fitting {
    /// The font size last fitted, if one was.
    pub fn font(&self) -> Option<f32> {
        self.font
    }

    /// The fitted font as a share of the person's, in whole percent; 100 when not fitted.
    pub fn percent(&self) -> u16 {
        match (self.font, self.layout) {
            (Some(font), Some(layout)) if layout.person > 0. => {
                (font / layout.person * 100.).round() as u16
            }
            _ => 100,
        }
    }

    /// Starts over, as when the terminal is no longer pinned.
    pub fn reset(&mut self) {
        *self = Fitting::default();
    }

    /// One step of `fitFont`: the font size to try next for `layout`, the grid having
    /// measured `drawn` (width, height) at the size it was last given.
    pub fn step(&mut self, layout: Layout, drawn: (f32, f32)) -> f32 {
        if self.layout != Some(layout) {
            self.layout = Some(layout);
            self.overflows_from = None;
        }
        let current = self.font.unwrap_or(layout.person);
        let room = (layout.room.0 / drawn.0.max(f32::EPSILON))
            .min(layout.room.1 / drawn.1.max(f32::EPSILON));
        if room < 1. {
            self.overflows_from = Some(self.overflows_from.map_or(current, |at| at.min(current)));
        }
        let mut wanted = ((current * room) / STEP).floor() * STEP;
        if let Some(overflows) = self.overflows_from {
            wanted = wanted.min(overflows - STEP);
        }
        let next = wanted.min(layout.person).max(FLOOR);
        self.font = Some(next);
        next
    }

    /// Fits `layout`, `measure` giving the grid's size at a font size: stepped until the size
    /// holds, as the dashboard's observer steps it at each resize.
    pub fn fit(&mut self, layout: Layout, mut measure: impl FnMut(f32) -> (f32, f32)) -> f32 {
        let mut font = self.font.unwrap_or(layout.person);
        for _ in 0..32 {
            let next = self.step(layout, measure(font));
            if next.to_bits() == font.to_bits() {
                break;
            }
            font = next;
        }
        font
    }
}

/// The most cells at `metrics` a `room` (width, height) holds: the size that keeps the
/// person's font at 100 %.
pub fn cells_in(room: (f32, f32), metrics: Metrics) -> (u16, u16) {
    let count = |room: f32, cell: Pixels| {
        let cell = f32::from(cell).max(f32::EPSILON);
        let mut count = (room / cell).floor();
        // The fit measures the cells times the cell, so a count that a rounding puts past
        // the room would draw smaller.
        while count > 0. && count * cell > room {
            count -= 1.;
        }
        let range = koyo_term_control::PIN_CELLS;
        count.clamp(f32::from(*range.start()), f32::from(*range.end())) as u16
    };
    (
        count(room.0, metrics.cell_width),
        count(room.1, metrics.cell_height),
    )
}

/// Where a pinned grid of `cells` in `metrics` sits in `bounds`: centred across, at the top,
/// snapped to device pixels; against the left edge when it's wider, so its right side is
/// what runs past.
pub fn origin(
    bounds: Bounds<Pixels>,
    cells: (u16, u16),
    metrics: Metrics,
    scale: f32,
) -> Point<Pixels> {
    let width = f32::from(metrics.cell_width) * f32::from(cells.0);
    let spare = ((f32::from(bounds.size.width) - width) / 2.).max(0.);
    let snap = |v: f32| (v * scale).round() / scale;
    bounds.origin + point(px(snap(spare)), px(0.))
}

/// The grid's extent from `origin`.
pub fn extent(origin: Point<Pixels>, cells: (u16, u16), metrics: Metrics) -> Bounds<Pixels> {
    let width = metrics.cell_width * f32::from(cells.0);
    let height = metrics.cell_height * f32::from(cells.1);
    Bounds::new(origin, size(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font whose cell is 0.6 by 1.2 of its size, its height snapped to whole pixels as a
    /// font's metrics round: what `fit` measures the grid with.
    fn grid_at(cells: (u16, u16)) -> impl FnMut(f32) -> (f32, f32) {
        move |font| {
            let width = font * 0.6;
            let height = (font * 1.2).ceil();
            (width * f32::from(cells.0), height * f32::from(cells.1))
        }
    }

    /// `font` is `expected`, to the bit: fitted sizes are whole steps, never approximations.
    fn same(font: f32, expected: f32) {
        assert_eq!(
            font.to_bits(),
            expected.to_bits(),
            "{font} px, not {expected} px"
        );
    }

    fn layout(room: (f32, f32), cells: (u16, u16)) -> Layout {
        Layout {
            room,
            cells,
            person: 13.,
        }
    }

    #[test]
    fn a_grid_that_fits_keeps_the_persons_font_and_never_grows_past_it() {
        let mut fitting = Fitting::default();
        let roomy = layout((2000., 2000.), (80, 24));
        same(fitting.fit(roomy, grid_at((80, 24))), 13.);
        assert_eq!(fitting.percent(), 100);
    }

    #[test]
    fn a_grid_too_big_is_drawn_smaller_in_quarter_pixel_steps_and_fits() {
        let mut fitting = Fitting::default();
        let cells = (200, 60);
        let tight = layout((910., 700.), cells);
        let font = fitting.fit(tight, grid_at(cells));
        assert!(font < 13.);
        same((font / STEP).round() * STEP, font);
        let (width, height) = grid_at(cells)(font);
        assert!(
            width <= 910. && height <= 700.,
            "{width}x{height} at {font}"
        );
        // A step bigger would overflow.
        let (width, height) = grid_at(cells)(font + STEP);
        assert!(
            width > 910. || height > 700.,
            "{font} isn't the largest that fits"
        );
        assert_eq!(fitting.percent(), (font / 13. * 100.).round() as u16);
    }

    #[test]
    fn the_font_never_goes_under_four_pixels_and_a_grid_too_big_even_then_stays_there() {
        let mut fitting = Fitting::default();
        let cells = (1000, 1000);
        let font = fitting.fit(layout((800., 600.), cells), grid_at(cells));
        same(font, FLOOR);
    }

    #[test]
    fn a_size_that_overflowed_isnt_tried_again_until_the_layout_changes() {
        let mut fitting = Fitting::default();
        let cells = (100, 30);
        // At 10.25 px the grid overflows though a step's arithmetic says it fits, as a font's
        // metrics rounding up makes it.
        let rounding = |font: f32| {
            let up = if font.to_bits() == 10.25f32.to_bits() {
                1.05
            } else {
                1.
            };
            (font * 60. * up, font * 36.)
        };
        let room = layout((620., 400.), cells);
        let font = fitting.fit(room, rounding);
        // 10.25 overflowed, so the step under it.
        same(font, 10.);
        // Asked again for the same layout, it doesn't go back to 10.25.
        same(fitting.fit(room, rounding), 10.);
        same(fitting.step(room, rounding(10.)), 10.);
        // A new room forgets what overflowed.
        let wider = layout((621., 400.), cells);
        same(fitting.fit(wider, |font| (font * 60., font * 36.)), 10.25);
    }

    #[test]
    fn a_grid_grows_back_to_the_persons_font_when_the_room_does() {
        let mut fitting = Fitting::default();
        let cells = (120, 40);
        let small = fitting.fit(layout((500., 400.), cells), grid_at(cells));
        assert!(small < 13.);
        let big = fitting.fit(layout((3000., 3000.), cells), grid_at(cells));
        same(big, 13.);
        fitting.reset();
        assert_eq!(fitting.font(), None);
        assert_eq!(fitting.percent(), 100);
    }

    fn metrics(width: f32, height: f32) -> Metrics {
        Metrics {
            cell_width: px(width),
            cell_height: px(height),
            baseline: px(height * 0.8),
            underline: px(height * 0.9),
            stroke: px(1.),
        }
    }

    #[test]
    fn the_grid_is_centred_across_at_the_top_or_against_the_left_when_too_wide() {
        let bounds = Bounds::new(point(px(100.), px(50.)), size(px(1000.), px(600.)));
        let cell = metrics(8., 16.);
        let centred = origin(bounds, (80, 24), cell, 2.);
        assert_eq!(centred, point(px(100. + 180.), px(50.)));
        let wide = origin(bounds, (200, 24), cell, 2.);
        assert_eq!(wide, point(px(100.), px(50.)));
        // Snapped to device pixels: 81 columns of 8.25 px leave 165.875 px a side, 166 at 2x.
        let odd = origin(bounds, (81, 24), metrics(8.25, 16.), 2.);
        assert_eq!(odd.x, px(100. + 166.));
    }

    #[test]
    fn cells_map_exactly_in_a_pinned_grid_drawn_smaller_and_off_centre() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(600.)));
        let small = metrics(4.5, 9.);
        let at = origin(bounds, (120, 40), small, 2.);
        let grid = crate::element::Grid {
            origin: at,
            metrics: small,
            cols: 120,
            rows: 40,
            scale: 2.,
        };
        let inside = at + point(px(4.5 * 10. + 1.), px(9. * 3. + 1.));
        assert_eq!(
            grid.cell_at(inside),
            koyo_term_engine::GridPoint { col: 10, row: 3 }
        );
        // Left of the grid and below it clamp to its first column and last row.
        let outside = point(px(1.), px(590.));
        assert_eq!(
            grid.cell_at(outside),
            koyo_term_engine::GridPoint { col: 0, row: 39 }
        );
        assert_eq!(
            grid.device(inside),
            ((4.5 * 10. + 1.) * 2., (9. * 3. + 1.) * 2.)
        );
        let geometry = grid.geometry();
        assert_eq!((geometry.cell_width, geometry.cell_height), (9, 18));
        assert_eq!((geometry.width, geometry.height), (9 * 120, 18 * 40));
    }

    #[test]
    fn the_cells_a_room_holds_are_drawn_there_at_the_persons_font() {
        // A 968×520 room at 8×16 cells holds 121×32.
        let room = (968., 520.);
        let cells = cells_in(room, metrics(8., 16.));
        assert_eq!(cells, (121, 32));
        let mut fitting = Fitting::default();
        let at = |font: f32| {
            let cell = (font / 13. * 8., font / 13. * 16.);
            (cell.0 * f32::from(cells.0), cell.1 * f32::from(cells.1))
        };
        same(fitting.fit(layout(room, cells), at), 13.);
        assert_eq!(fitting.percent(), 100);
        // A room too small for two cells still holds the least a pin has.
        assert_eq!(cells_in((3., 3.), metrics(8., 16.)), (2, 2));
    }

    #[test]
    fn a_percent_is_of_the_persons_font_in_whole_steps_and_never_under_the_floor() {
        same(font_at(100, 13.), 13.);
        same(font_at(75, 13.), 9.75);
        same(font_at(50, 13.), 6.5);
        same(font_at(50, 13.5), 6.75);
        same(font_at(10, 13.), FLOOR);
    }
}

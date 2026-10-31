//! Box drawing and block elements drawn as rectangles at device-pixel size, never from the
//! font, so lines join across cells and blocks tile without seams (`docs/terminals.md` 7.2,
//! after Ghostty's `src/font/sprite`). Light and heavy lines and the block elements are
//! drawn; dashes, arcs, double lines and diagonals come from the font.

use gpui_kit::{Bounds, Hsla, Pixels, Window, fill, point, px, size};

/// The weight of each arm of a box-drawing character, up, right, down, left: 0 none,
/// 1 light, 2 heavy. `None` for the characters the font draws.
fn arms(c: char) -> Option<[u8; 4]> {
    let code = c as u32;
    let spec: &[u8; 4] = match code {
        0x2500 => b"0101",
        0x2501 => b"0202",
        0x2502 => b"1010",
        0x2503 => b"2020",
        0x250C..=0x254B => &CORNERS[(code - 0x250C) as usize],
        0x2574..=0x257F => &HALVES[(code - 0x2574) as usize],
        _ => return None,
    };
    Some(spec.map(|digit| digit - b'0'))
}

/// U+250C to U+254B: corners, tees and crosses, in code order.
const CORNERS: [[u8; 4]; 64] = [
    *b"0110", *b"0210", *b"0120", *b"0220", // ┌ ┍ ┎ ┏
    *b"0011", *b"0012", *b"0021", *b"0022", // ┐ ┑ ┒ ┓
    *b"1100", *b"1200", *b"2100", *b"2200", // └ ┕ ┖ ┗
    *b"1001", *b"1002", *b"2001", *b"2002", // ┘ ┙ ┚ ┛
    *b"1110", *b"1210", *b"2110", *b"1120", // ├ ┝ ┞ ┟
    *b"2120", *b"2210", *b"1220", *b"2220", // ┠ ┡ ┢ ┣
    *b"1011", *b"1012", *b"2011", *b"1021", // ┤ ┥ ┦ ┧
    *b"2021", *b"2012", *b"1022", *b"2022", // ┨ ┩ ┪ ┫
    *b"0111", *b"0112", *b"0211", *b"0212", // ┬ ┭ ┮ ┯
    *b"0121", *b"0122", *b"0221", *b"0222", // ┰ ┱ ┲ ┳
    *b"1101", *b"1102", *b"1201", *b"1202", // ┴ ┵ ┶ ┷
    *b"2101", *b"2102", *b"2201", *b"2202", // ┸ ┹ ┺ ┻
    *b"1111", *b"1112", *b"1211", *b"1212", // ┼ ┽ ┾ ┿
    *b"2111", *b"1121", *b"2121", *b"2112", // ╀ ╁ ╂ ╃
    *b"2211", *b"1122", *b"1221", *b"2212", // ╄ ╅ ╆ ╇
    *b"1222", *b"2122", *b"2221", *b"2222", // ╈ ╉ ╊ ╋
];

/// U+2574 to U+257F: half lines.
const HALVES: [[u8; 4]; 12] = [
    *b"0001", *b"1000", *b"0100", *b"0010", // ╴ ╵ ╶ ╷
    *b"0002", *b"2000", *b"0200", *b"0020", // ╸ ╹ ╺ ╻
    *b"0201", *b"1020", *b"0102", *b"2010", // ╼ ╽ ╾ ╿
];

/// Draws `c` in `cell` (`cells` wide) in `color`; `false` when it isn't a sprite.
pub fn paint(c: char, cell: Bounds<Pixels>, color: Hsla, window: &mut Window) -> bool {
    let scale = window.scale_factor();
    let snap = |v: Pixels| px((f32::from(v) * scale).round() / scale);
    let device = px(1. / scale);
    if let Some(arms) = arms(c) {
        // A light line is about an eighth of the cell's width, at least a device pixel.
        let light = snap(cell.size.width / 8.).max(device);
        let heavy = light * 2.;
        let (left, top) = (cell.origin.x, cell.origin.y);
        let (width, height) = (cell.size.width, cell.size.height);
        let (mid_x, mid_y) = (snap(left + width / 2.), snap(top + height / 2.));
        let thick = |weight: u8| if weight == 2 { heavy } else { light };
        // How far past the centre an arm reaches, so arms of either weight meet squarely.
        let reach = arms
            .iter()
            .map(|&weight| {
                if weight == 0 {
                    px(0.)
                } else {
                    thick(weight) / 2.
                }
            })
            .fold(px(0.), Ord::max);
        let [up, right, down, left_arm] = arms;
        let mut line = |bounds: Bounds<Pixels>| window.paint_quad(fill(bounds, color));
        if up > 0 {
            let stroke = thick(up);
            line(Bounds::new(
                point(mid_x - stroke / 2., top),
                size(stroke, mid_y + reach - top),
            ));
        }
        if down > 0 {
            let stroke = thick(down);
            line(Bounds::new(
                point(mid_x - stroke / 2., mid_y - reach),
                size(stroke, top + height - (mid_y - reach)),
            ));
        }
        if left_arm > 0 {
            let stroke = thick(left_arm);
            line(Bounds::new(
                point(left, mid_y - stroke / 2.),
                size(mid_x + reach - left, stroke),
            ));
        }
        if right > 0 {
            let stroke = thick(right);
            line(Bounds::new(
                point(mid_x - reach, mid_y - stroke / 2.),
                size(left + width - (mid_x - reach), stroke),
            ));
        }
        return true;
    }
    let Some(parts) = block(c) else { return false };
    for part in parts {
        let Part {
            left,
            top,
            right,
            bottom,
            alpha,
        } = *part;
        let x0 = snap(cell.origin.x + cell.size.width * left);
        let x1 = snap(cell.origin.x + cell.size.width * right);
        let y0 = snap(cell.origin.y + cell.size.height * top);
        let y1 = snap(cell.origin.y + cell.size.height * bottom);
        window.paint_quad(fill(
            Bounds::new(point(x0, y0), size(x1 - x0, y1 - y0)),
            color.opacity(alpha),
        ));
    }
    true
}

/// A rectangle of a block element, in fractions of the cell.
#[derive(Clone, Copy)]
struct Part {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    alpha: f32,
}

const fn part(left: f32, top: f32, right: f32, bottom: f32) -> Part {
    Part {
        left,
        top,
        right,
        bottom,
        alpha: 1.,
    }
}

const UPPER_HALF: Part = part(0., 0., 1., 0.5);
const RIGHT_HALF: Part = part(0.5, 0., 1., 1.);
const UPPER_EIGHTH: Part = part(0., 0., 1., 0.125);
const RIGHT_EIGHTH: Part = part(0.875, 0., 1., 1.);
const LIGHT_SHADE: Part = Part {
    alpha: 0.25,
    ..part(0., 0., 1., 1.)
};
const MEDIUM_SHADE: Part = Part {
    alpha: 0.5,
    ..part(0., 0., 1., 1.)
};
const DARK_SHADE: Part = Part {
    alpha: 0.75,
    ..part(0., 0., 1., 1.)
};
const UPPER_LEFT: Part = part(0., 0., 0.5, 0.5);
const UPPER_RIGHT: Part = part(0.5, 0., 1., 0.5);
const LOWER_LEFT: Part = part(0., 0.5, 0.5, 1.);
const LOWER_RIGHT: Part = part(0.5, 0.5, 1., 1.);

/// U+2580 to U+259F.
fn block(c: char) -> Option<&'static [Part]> {
    const LOWER: [Part; 8] = [
        part(0., 0.875, 1., 1.),
        part(0., 0.75, 1., 1.),
        part(0., 0.625, 1., 1.),
        part(0., 0.5, 1., 1.),
        part(0., 0.375, 1., 1.),
        part(0., 0.25, 1., 1.),
        part(0., 0.125, 1., 1.),
        part(0., 0., 1., 1.),
    ];
    const LEFT: [Part; 7] = [
        part(0., 0., 0.875, 1.),
        part(0., 0., 0.75, 1.),
        part(0., 0., 0.625, 1.),
        part(0., 0., 0.5, 1.),
        part(0., 0., 0.375, 1.),
        part(0., 0., 0.25, 1.),
        part(0., 0., 0.125, 1.),
    ];
    let parts: &'static [Part] = match c {
        '\u{2580}' => &[UPPER_HALF],
        '\u{2581}'..='\u{2588}' => std::slice::from_ref(&LOWER[(c as u32 - 0x2581) as usize]),
        '\u{2589}'..='\u{258F}' => std::slice::from_ref(&LEFT[(c as u32 - 0x2589) as usize]),
        '\u{2590}' => &[RIGHT_HALF],
        '\u{2591}' => &[LIGHT_SHADE],
        '\u{2592}' => &[MEDIUM_SHADE],
        '\u{2593}' => &[DARK_SHADE],
        '\u{2594}' => &[UPPER_EIGHTH],
        '\u{2595}' => &[RIGHT_EIGHTH],
        '\u{2596}' => &[LOWER_LEFT],
        '\u{2597}' => &[LOWER_RIGHT],
        '\u{2598}' => &[UPPER_LEFT],
        '\u{2599}' => &[UPPER_LEFT, LOWER_LEFT, LOWER_RIGHT],
        '\u{259A}' => &[UPPER_LEFT, LOWER_RIGHT],
        '\u{259B}' => &[UPPER_LEFT, UPPER_RIGHT, LOWER_LEFT],
        '\u{259C}' => &[UPPER_LEFT, UPPER_RIGHT, LOWER_RIGHT],
        '\u{259D}' => &[UPPER_RIGHT],
        '\u{259E}' => &[UPPER_RIGHT, LOWER_LEFT],
        '\u{259F}' => &[UPPER_RIGHT, LOWER_LEFT, LOWER_RIGHT],
        _ => return None,
    };
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_know_their_arms() {
        assert_eq!(arms('─'), Some([0, 1, 0, 1]));
        assert_eq!(arms('┏'), Some([0, 2, 2, 0]));
        assert_eq!(arms('┼'), Some([1, 1, 1, 1]));
        assert_eq!(arms('╋'), Some([2, 2, 2, 2]));
        assert_eq!(arms('┡'), Some([2, 2, 1, 0]));
        assert_eq!(arms('╿'), Some([2, 0, 1, 0]));
        // Arcs, dashes and double lines are the font's.
        assert_eq!(arms('╭'), None);
        assert_eq!(arms('┄'), None);
        assert_eq!(arms('═'), None);
    }
}

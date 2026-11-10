//! Glass over what the theme doesn't colour, a web page: dark over a light page and light over a
//! dark one, so it stands off the page whatever the page's colour, and what's on it, drawn in the
//! palette of the glass's own tone (`palette_for`), reads. Over the app's own surfaces glass keeps
//! the theme's tone, as they're in its colours already.

use gpui_kit::{App, Hsla};

use super::{Palette, palette, palette_for};

/// The luminance at which a page counts as light: where black text and white read as well as each
/// other on it, WCAG's contrast ratio being the same for both there.
const MIDDLE: f32 = 0.18;
/// How far past `MIDDLE` a page goes before glass that's one tone over it turns the other: a page
/// near the middle, or changing a little under it, doesn't make it flicker.
const MARGIN: f32 = 0.05;
/// How much of the page the tint of glass hides: enough that its tone holds over a page of the
/// other one.
const TINT: f32 = 0.78;

/// Whether glass over a page `lightness` light, its mean relative luminance from 0 to 1, is dark,
/// given whether it `was`, if it was there before.
pub fn dark_over(lightness: f32, was: Option<bool>) -> bool {
    let middle = match was {
        Some(true) => MIDDLE - MARGIN,
        Some(false) => MIDDLE + MARGIN,
        None => MIDDLE,
    };
    lightness > middle
}

/// The palette of what's drawn on glass dark if `dark`, light if not, and the glass's tint over
/// the blur of the page, its card's colour deep enough to hold the tone over a page of the other
/// one; the theme's palette and no tint where `dark` is none, and the glass is the theme's.
pub fn look(dark: Option<bool>, cx: &App) -> (Palette, Option<Hsla>) {
    match dark {
        Some(dark) => {
            let p = palette_for(dark, cx);
            (p, Some(p.card.alpha(TINT)))
        }
        None => (palette(cx), None),
    }
}

#[cfg(test)]
mod tests {
    use super::dark_over;

    #[test]
    fn glass_is_dark_over_a_light_page_and_light_over_a_dark_one() {
        assert!(dark_over(1., None), "white");
        assert!(dark_over(0.4, None), "light grey");
        assert!(!dark_over(0., None), "black");
        assert!(!dark_over(0.06, None), "dark grey");
    }

    #[test]
    fn a_page_near_the_middle_leaves_the_glass_as_it_was() {
        assert!(dark_over(0.16, Some(true)));
        assert!(!dark_over(0.2, Some(false)));
        assert!(!dark_over(0.1, Some(true)), "well past it, the glass turns");
        assert!(
            dark_over(0.26, Some(false)),
            "well past it, the glass turns"
        );
    }
}

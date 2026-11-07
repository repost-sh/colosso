//! How every text view draws what markdown sets apart from its words: code and tables in the
//! palette's washes, shades of the theme laid thin over the card, rather than the kit's solid
//! greys, which sat on a card of any colour as slabs of another; inline code off the system's
//! accent; code blocks sized in rems, to grow with a chat's text.

use gpui_kit::base::text::editor_host::RenderContext;
use gpui_kit::base::{TextViewDefaults, TextViewStyle};
use gpui_kit::component::Theme;
use gpui_kit::{App, HighlightStyle, Styled as _, rems};

use super::{Palette, palette};
use crate::settings;

/// How big a code block's text is, in rems: the kit's 13 pixels, where a rem is 16.
const CODE_BLOCK_TEXT: f32 = 13. / 16.;

pub(super) fn init(cx: &mut App) {
    // The kit puts its own style back as the theme changes, and the gradient theme the washes
    // are tinted by is among the settings.
    cx.observe_global::<Theme>(follow_palette).detach();
    settings::observe_app(cx, follow_palette).detach();
    follow_palette(cx);
}

/// Puts the text views on the palette in use, unless they're there already: a change to a
/// setting that's no colour sets nothing.
fn follow_palette(cx: &mut App) {
    let style = RenderContext::from_app(cx).style().clone();
    let drawn = on_palette(style.clone(), palette(cx));
    if drawn != style {
        TextViewDefaults::global(cx).with_style(drawn).install(cx);
    }
}

/// `style` in `p`'s colours: a code block and a table's body faintly washed, a table's head and
/// code in a line more, the lines round and through them in the theme's.
fn on_palette(style: TextViewStyle, p: Palette) -> TextViewStyle {
    let inline_code = HighlightStyle {
        background_color: Some(p.wash),
        ..style.inline_code()
    };
    let block = style.code_block().clone().text_size(rems(CODE_BLOCK_TEXT));
    let table = style.table().clone().bg(p.faint_wash);
    let head = style.table_head().clone().bg(p.wash).text_color(p.text);
    style
        .with_code_background(p.faint_wash)
        .with_border(p.wash_line)
        .with_inline_code(inline_code)
        .with_code_block(block)
        .with_table(table)
        .with_table_head(head)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{StyleRefinement, TestAppContext, point};

    use super::*;
    use crate::gradient;
    use crate::theme::{ThemePreference, set_preference};

    /// The app as it starts, in the dark theme.
    fn dark(cx: &mut TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| set_preference(ThemePreference::Dark, cx));
    }

    /// The palette in use, once it's checked the text views draw in it.
    fn drawn_on_palette(cx: &mut TestAppContext) -> Palette {
        cx.read(|cx| {
            let (style, p) = (RenderContext::from_app(cx).style().clone(), palette(cx));
            let fill = |colour| StyleRefinement::default().bg(colour).background;
            assert_eq!(style.inline_code().background_color, Some(p.wash));
            assert_eq!(style.code_background(), p.faint_wash);
            assert_eq!(style.border(), p.wash_line);
            assert_eq!(style.table().background, fill(p.faint_wash));
            assert_eq!(style.table_head().background, fill(p.wash));
            p
        })
    }

    #[gpui_kit::test]
    fn code_and_tables_stay_on_the_palette_whichever_appearance_it_takes(cx: &mut TestAppContext) {
        dark(cx);
        let dark = drawn_on_palette(cx);
        // The kit puts its own back as the theme changes.
        cx.update(|cx| set_preference(ThemePreference::Light, cx));
        let light = drawn_on_palette(cx);
        assert_ne!(dark.wash, light.wash);
        for p in [dark, light] {
            assert!(
                p.wash.a < 1. && p.faint_wash.a < 1.,
                "the card shows through"
            );
        }
    }

    #[gpui_kit::test]
    fn code_and_tables_take_the_gradient_theme_s_colour(cx: &mut TestAppContext) {
        dark(cx);
        let gray = drawn_on_palette(cx);
        assert!(
            gray.wash.s.abs() < f32::EPSILON,
            "gray with no colour picked"
        );
        // A dot on the disc's right, far out: a red.
        cx.update(|cx| gradient::update(cx, |theme| theme.move_primary(point(300., 180.))));
        let red = drawn_on_palette(cx);
        assert!(
            red.wash.s > 0.3,
            "the wash takes the theme's colour: {:?}",
            red.wash
        );
        assert!(
            (red.wash.l - gray.wash.l).abs() < f32::EPSILON,
            "at its own lightness"
        );
    }
}

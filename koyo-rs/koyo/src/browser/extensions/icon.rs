//! An extension's icon as it's drawn (`docs/extensions.md` 5): its own picture at 16 points,
//! or the first letter of its name on a round; greyed where its action is off; and its badge
//! at the lower right, as Chrome draws one.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Div, FontWeight, Hsla, IntoElement as _, ObjectFit, ParentElement as _, Styled as _,
    StyledImage as _, div, img,
    px, rgba,
};

use super::{Badge, Shown};

/// The picture's side, and the room a button gives it.
pub(in crate::browser) const SIDE: f32 = 16.;
pub(in crate::browser) const BUTTON: f32 = 24.;
/// A badge with no colour of the extension's is Chrome's blue, and its text white.
const BADGE: u32 = 0x1a73_e8ff;

/// `shown`'s icon with its badge, in a square of [`BUTTON`]. `text` colours a letter, on
/// `round`.
pub(in crate::browser) fn face(shown: &Shown, text: Hsla, round: Hsla) -> Div {
    let picture = match &shown.icon {
        Some(icon) => img(icon.clone())
            .size(px(SIDE))
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
        None => div()
            .size(px(SIDE))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .bg(round)
            .text_size(px(10.))
            .font_weight(FontWeight::BOLD)
            .text_color(text)
            .child(letter(&shown.name))
            .into_any_element(),
    };
    div()
        .relative()
        .size(px(BUTTON))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(div().when(!shown.enabled, |icon| icon.opacity(0.4)).child(picture))
        .children(shown.badge.as_ref().map(badge))
}

/// The letter an extension with no icon shows.
fn letter(name: &str) -> String {
    (name.chars().find(|c| c.is_alphanumeric()))
        .map_or_else(|| "?".into(), |c| c.to_uppercase().collect())
}

/// A badge's colours: the extension's, or Chrome's blue; and its text's, or whichever of
/// white and black reads better on that.
fn colors(badge: &Badge) -> (Hsla, Hsla) {
    let back = badge.color.filter(|color| color[3] != 0);
    let fill = back.map_or(BADGE, pack);
    let text = match badge.text_color.filter(|color| color[3] != 0) {
        Some(text) => pack(text),
        None if back.is_some_and(is_light) => 0x0000_00ff,
        None => 0xffff_ffff,
    };
    (rgba(fill).into(), rgba(text).into())
}

fn pack([r, g, b, a]: [u8; 4]) -> u32 {
    u32::from_be_bytes([r, g, b, a])
}

/// Whether black text reads better on `color` than white.
fn is_light([r, g, b, _]: [u8; 4]) -> bool {
    // Perceived brightness, as the eye weighs the three.
    299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b) > 150_000
}

/// What a badge says in the room it has: Chrome shows four characters at most.
fn label(text: &str) -> String {
    text.chars().take(4).collect()
}

fn badge(badge: &Badge) -> Div {
    let (fill, text) = colors(badge);
    div()
        .absolute()
        .right(px(0.))
        .bottom(px(1.))
        .h(px(10.))
        .min_w(px(10.))
        .px(px(2.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .bg(fill)
        .text_color(text)
        .text_size(px(8.))
        .line_height(px(10.))
        .font_weight(FontWeight::BOLD)
        .child(label(&badge.text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn badge_of(color: Option<[u8; 4]>, text_color: Option<[u8; 4]>) -> Badge {
        Badge { text: "1".into(), color, text_color }
    }

    #[test]
    fn a_badge_is_chromes_blue_unless_the_extension_chose() {
        let white: Hsla = rgba(0xffff_ffff).into();
        let black: Hsla = rgba(0x0000_00ff).into();
        let (fill, text) = colors(&badge_of(None, None));
        assert_eq!((fill, text), (rgba(BADGE).into(), white));
        // A colour that's wholly clear is none chosen.
        assert_eq!(colors(&badge_of(Some([0, 0, 0, 0]), None)).0, rgba(BADGE).into());
        // On a light colour of its own the text is black; on a dark one white.
        assert_eq!(colors(&badge_of(Some([255, 230, 0, 255]), None)).1, black);
        assert_eq!(colors(&badge_of(Some([180, 0, 0, 255]), None)).1, white);
        // The extension's own text colour wins.
        let (_, text) = colors(&badge_of(Some([255, 230, 0, 255]), Some([255, 0, 0, 255])));
        assert_eq!(text, rgba(0xff00_00ff).into());
    }

    #[test]
    fn a_badge_says_four_characters_and_an_iconless_extension_a_letter() {
        assert_eq!(label("12345"), "1234");
        assert_eq!(label("9"), "9");
        assert_eq!(letter("uBlock"), "U");
        assert_eq!(letter("  1Password"), "1");
        assert_eq!(letter(""), "?");
    }
}

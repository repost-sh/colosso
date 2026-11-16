//! The backdrop behind a chat (`docs/chat-backdrop.md`): the part of the screen's wallpaper
//! the chat's pane covers, dithered in its own colours, faint, and fading out towards the
//! bottom, as Monodrive's Dither effect looks (github.com/donnes/monodrive, MIT).

mod astc;
mod element;
mod layout;
mod make;
mod screens;
#[cfg(test)]
mod tests;
mod texture;
#[cfg(feature = "visual-check")]
pub mod visual;
mod wallpaper;
mod watch;

use gpui_kit::{AnyElement, App, Hsla, IntoElement as _, ParentElement, Styled};

pub use astc::Blocks;
pub use screens::memory;
pub use watch::init;

use crate::native::liquid;
use crate::settings;
use crate::theme::Palette;

/// The backdrop behind a chat, filling its parent, which must be `relative`; none while it's
/// turned off, and the card shows.
pub fn backdrop(cx: &App) -> Option<AnyElement> {
    let settings = settings::get(cx).chat_backdrop;
    (settings.on).then(|| element::Picture { settings }.into_any_element())
}

/// `surface`, floating over a chat as [`liquid::floating`] has it, but see-through over the
/// backdrop where there's no glass ([`fill`]).
pub fn floating<E: ParentElement + Styled>(
    surface: E,
    glass: Option<AnyElement>,
    p: Palette,
    cx: &App,
) -> E {
    liquid::floating_filled(surface, glass, fill(p, cx), p)
}

/// The fill of what floats over a chat where there's no glass: over the backdrop, the card's
/// own colour, as see-through as the card is, so the backdrop shows through as it does through
/// glass; solid otherwise (`Palette::floating`).
fn fill(p: Palette, cx: &App) -> Hsla {
    if settings::get(cx).chat_backdrop.on {
        p.card
    } else {
        p.floating()
    }
}

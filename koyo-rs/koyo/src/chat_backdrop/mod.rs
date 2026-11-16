//! The backdrop behind a chat (`docs/chat-backdrop.md`): the part of the screen's wallpaper
//! the chat's pane covers, dimmed towards the card's colour, under a fixed grid of dots, and
//! blurred towards the bottom.

mod astc;
mod blur;
mod dots;
mod element;
mod layout;
mod make;
mod screens;
mod strips;
#[cfg(test)]
mod tests;
mod texture;
mod wallpaper;
mod watch;

use gpui_kit::{AnyElement, App, Hsla};

pub use astc::Blocks;
pub use screens::memory;
pub use watch::init;

use crate::settings;
use crate::theme::Palette;

/// The backdrop behind a chat, filling its parent, which must be `relative`; none while it's
/// turned off, and the card shows.
pub fn backdrop(cx: &App) -> Option<AnyElement> {
    let settings = settings::get(cx).chat_backdrop;
    settings.on.then(|| strips::backdrop(settings))
}

/// The fill of what floats over a chat where there's no glass: over the backdrop, the card's
/// own colour, as see-through as the card is, so the backdrop shows through as it does through
/// glass; solid otherwise (`Palette::floating`).
pub fn fill(p: Palette, cx: &App) -> Hsla {
    if settings::get(cx).chat_backdrop.on {
        p.card
    } else {
        p.floating()
    }
}

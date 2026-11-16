//! The backdrop behind a chat (`docs/chat-backdrop.md`): the part of the screen's wallpaper
//! the chat's pane covers, dimmed towards the card's colour, under a fixed grid of dots, and
//! blurred towards the bottom.

mod astc;
mod element;
mod layout;
mod make;
mod screens;
#[cfg(test)]
mod tests;
mod texture;
mod wallpaper;

use gpui_kit::{AnyElement, App, IntoElement as _};

pub use astc::Blocks;

use crate::settings;

/// The backdrop behind a chat, filling its parent, which must be `relative`; none while it's
/// turned off, and the card shows.
pub fn backdrop(cx: &App) -> Option<AnyElement> {
    let on = settings::get(cx).chat_backdrop.on;
    on.then(|| element::Picture.into_any_element())
}

//! The backdrop behind a chat (`docs/chat-backdrop.md`): the part of the screen's wallpaper
//! the chat's pane covers, dimmed towards the card's colour, under a fixed grid of dots, and
//! blurred towards the bottom.

#![expect(dead_code, reason = "the chat draws it next (docs/chat-backdrop.md 10)")]

mod astc;
mod layout;
mod make;
mod wallpaper;

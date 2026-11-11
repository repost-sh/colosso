//! How many lines a text wraps to, measured before what shows it opens, as a window has to be
//! sized then: a dialog's heading, a menu's rows, the accounts panel's notes.

use gpui_kit::{Pixels, SharedString, TextStyle, Window};

/// How many lines `text` in `style` at `size` wraps to across `width`; one when it can't be
/// shaped.
pub fn wrapped_lines(
    text: &SharedString,
    style: &TextStyle,
    size: Pixels,
    width: Pixels,
    window: &Window,
) -> usize {
    let run = style.to_run(text.len());
    (window.text_system())
        .shape_text(text.clone(), size, &[run], Some(width), None)
        .map_or(1, |lines| {
            (lines.iter())
                .map(|line| 1 + line.wrap_boundaries().len())
                .sum()
        })
}

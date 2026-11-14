//! The note over the composer, which says what's in the way.

use gpui_kit::{TestAppContext, VisualTestContext};

use super::chat;
use crate::test_support::draw;

/// Text straight in a row is as wide as it's long: a long reason ran out of the column.
#[gpui_kit::test]
fn a_long_note_wraps_at_the_columns_width(cx: &mut TestAppContext) {
    let chat = chat("chat-note-wraps", cx);
    let noted = |why: String, cx: &mut TestAppContext| {
        chat.composer.update(cx, |composer, cx| {
            composer.error = Some(why.into());
            cx.notify();
        });
        draw(chat.window, cx);
        let note = VisualTestContext::from_window(chat.window, cx).debug_bounds("chat-note");
        note.expect("the note is drawn")
    };
    let line = noted("That couldn't be sent.".to_owned(), cx);
    let long = noted("That couldn't be sent, and here is why. ".repeat(16), cx);
    let composer = VisualTestContext::from_window(chat.window, cx)
        .debug_bounds("chat-composer")
        .expect("the composer is drawn");
    assert!(
        long.size.height > line.size.height * 1.5,
        "it takes the lines it needs: {long:?} against {line:?}"
    );
    assert!(
        long.size.width <= composer.size.width,
        "inside the column: {long:?} in {composer:?}"
    );
}

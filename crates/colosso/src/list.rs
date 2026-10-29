//! What every list moved through by keyboard shares: the same keys in each, the same
//! steps along it, and how a row shows the bytes that matched.

use std::ops::Range;

use gpui_kit::{
    App, HighlightStyle, Hsla, KeyBinding, SharedString, StyledText, UniformListScrollHandle,
    actions,
};

actions!(
    list,
    [
        SelectPrevious,
        SelectNext,
        SelectFirst,
        SelectLast,
        PageUp,
        PageDown,
        PreviousFile,
        NextFile,
        Collapse,
        Expand,
        Activate
    ]
);

/// The keys that move along a list while what has `context` has the keyboard: the arrows, as
/// Emacs has them too, and a page at a time.
pub fn bind_keys(context: &'static str, cx: &mut App) {
    let context = Some(context);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, context),
        KeyBinding::new("ctrl-p", SelectPrevious, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("ctrl-n", SelectNext, context),
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
    ]);
}

/// Where a step of `by` rows from `at` lands in a list of `len`, stopping at either end; from
/// nothing, on the first row going down and the last going up. Nothing in an empty list.
pub fn stepped(at: Option<usize>, by: isize, len: usize) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match at {
        Some(at) => at.saturating_add_signed(by).min(last),
        None if by > 0 => 0,
        None => last,
    })
}

/// How many of a list's rows, `row` high, fit in it: a page's step.
pub fn page(scroll: &UniformListScrollHandle, row: f32) -> isize {
    let height = scroll.0.borrow().base_handle.bounds().size.height.as_f32();
    (height / row).floor().max(1.) as isize
}

/// `text` with its `matched` bytes in `colour`: what a search matched, or a place found.
pub fn styled(text: &SharedString, matched: &[Range<usize>], colour: Hsla) -> StyledText {
    let style = HighlightStyle {
        color: Some(colour),
        ..HighlightStyle::default()
    };
    let highlights = matched.iter().map(|range| (range.clone(), style));
    StyledText::new(text.clone()).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stop_at_the_ends_and_start_from_one() {
        assert_eq!(stepped(Some(3), 2, 10), Some(5));
        assert_eq!(stepped(Some(8), 5, 10), Some(9));
        assert_eq!(stepped(Some(1), -5, 10), Some(0));
        assert_eq!(stepped(None, 1, 10), Some(0));
        assert_eq!(stepped(None, -1, 10), Some(9));
        assert_eq!(stepped(Some(0), 1, 0), None);
    }
}

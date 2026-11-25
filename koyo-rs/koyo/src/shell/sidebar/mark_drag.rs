//! The spaces' marks dragged into a new order along the sidebar's foot (`docs/layout.md`,
//! Reordering spaces), by the drag that moves tabs and rows (`shell/reorder.rs`): the mark in
//! hand follows the pointer and the others slide aside, and let go it settles into the slot it's
//! over, the order kept for the next launch and the space shown staying shown. Pressed and let go
//! without moving, a mark shows its space, as a click; pulled well off the row it slides back,
//! and nothing changes.

use gpui_kit::{AnyElement, Axis, Context, px};

use crate::settings;
use crate::shell::{List, Reorder, Shell, on_move};

/// The spaces' `marks` in a row along the foot, in the spaces' order, each by its space's id and
/// taken up by the row as it's dragged sideways (`foot::mark`): it moves among the others, and
/// let go its space goes to the slot it's over.
pub(super) fn row(shell: &Shell, marks: Vec<(u64, AnyElement)>, cx: &Context<Shell>) -> Reorder {
    let on_drop = on_move(cx, |_, from, to, cx| {
        settings::update(cx, |settings| settings.move_space(from, to));
    });
    let list = (List::Spaces, Axis::Horizontal, true);
    Reorder::rows(list, marks, shell.mark_hand.clone(), px(0.), on_drop)
}

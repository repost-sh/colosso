//! A checkout's tabs as a list, along the strip over the panes in Orca and down the sidebar
//! in Arc: dragged, a tab moves among them or joins another side by side, and pulled off
//! the list it goes onto a side of a pane; a pane of a split dropped on a tab goes where
//! that tab is.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Along as _, AnyElement, AppContext as _, Axis, Context, Div, InteractiveElement as _,
    IntoElement as _, ParentElement as _, Pixels, Point, Stateful, StatefulInteractiveElement as _,
    Styled as _, px,
};

use super::Shell;
use super::reorder::{Drop, Reorder, TabHand};
use super::splits::{Dragged, DraggedPane};
use super::tab_face::{Face, split_chips};
use crate::model::Tab;
use crate::theme::Palette;

/// How much of the tab in hand shows while it's over a tab it would join.
const JOINING_OPACITY: f32 = 0.35;

/// The list of the checkout `(project, branch)`.
fn list_id((project, branch): (usize, usize)) -> u64 {
    ((project as u64) << 32) | branch as u64
}

/// The tabs of the checkout `at`, as `(project, branch)`, as `tab_row` makes them, in a
/// list along `axis`.
pub fn tab_list(
    hand: &Rc<TabHand>,
    (at, axis): ((usize, usize), Axis),
    tabs: Vec<(u64, AnyElement)>,
    gap: Pixels,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Reorder {
    let on_drop = cx.listener(move |this, drop: &Drop, _, cx| this.drop_in_list(at, *drop, cx));
    let list = (list_id(at), axis);
    Reorder::new(list, tabs, hand.clone(), gap, p.accent, Rc::new(on_drop))
}

/// The tab `tab` at `at`, as `(project, branch, tab)`, in its list along `axis`, keyed by
/// what it keeps as it moves: `row` showing a split's panes as chips, and a single pane as
/// `single` has it. Dragged, it's taken up by the list, and what's dropped on it from a
/// split goes where it is.
pub fn tab_row(
    row: Stateful<Div>,
    (at, tab): ((usize, usize, usize), &Tab),
    (axis, hand): (Axis, &Rc<TabHand>),
    p: Palette,
    cx: &mut Context<Shell>,
    single: impl FnOnce(Stateful<Div>, &Face, &mut Context<Shell>) -> Stateful<Div>,
) -> (u64, AnyElement) {
    let faces = Face::all(tab, cx);
    let row = match (tab, faces.first()) {
        (Tab::Single(_), Some(face)) => single(row, face, cx),
        // Along the strip a split's chips fill the row nearly to its edge; down the sidebar its
        // halves keep the row's own room, as a single pane does.
        _ => row
            .when(axis == Axis::Horizontal, |row| row.px(px(3.)))
            .child(split_chips(&faces, axis, p, cx)),
    };
    let key = tab.first().id;
    let row = tab_drag(row, (at, key), (axis, hand), faces, p, cx);
    (key, row.into_any_element())
}

/// The row of the tab `key` at `at`: dragged, it's taken up by the list, and what's dropped
/// on it from a split goes where it is.
fn tab_drag(
    row: Stateful<Div>,
    (at, key): ((usize, usize, usize), u64),
    (axis, hand): (Axis, &Rc<TabHand>),
    faces: Vec<Face>,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let list = list_id((at.0, at.1));
    let held = DraggedPane {
        what: Dragged::Tab(at.0, at.1, at.2),
        faces,
        hand: Some(hand.clone()),
        grab: Point::default(),
    };
    let lifted = hand.holding() == Some(key);
    let joining = lifted && hand.joining();
    let hand = hand.clone();
    row.debug_selector(move || format!("tab-{}-{}-{}", at.0, at.1, at.2))
        // In hand, it's lifted off the list, over the tabs it passes.
        .when(lifted, |row| row.bg(p.hover).shadow_sm())
        // Over the middle of another, it fades so the one it would join shows through.
        .when(joining, |row| row.opacity(JOINING_OPACITY))
        .on_drag(held, move |held, grab, _, cx| {
            hand.take(list, key, at.2, grab.along(axis));
            let chip = held.chip(grab);
            cx.new(|_| chip)
        })
        .drag_over::<DraggedPane>(move |style, held, _, _| match held.what {
            Dragged::Pane(_) => style.bg(p.hover),
            // A tab from another checkout's list would go here.
            Dragged::Tab(project, branch, _) if (project, branch) != (at.0, at.1) => {
                style.bg(p.hover)
            }
            Dragged::Tab(..) => style,
        })
        .on_drop(cx.listener(move |this, held: &DraggedPane, _, cx| {
            this.drop_on_strip(held, at, cx);
        }))
}

impl Shell {
    /// A tab of the checkout `(project, branch)` let go over its list, which then shows.
    fn drop_in_list(
        &mut self,
        (project, branch): (usize, usize),
        drop: Drop,
        cx: &mut Context<Self>,
    ) {
        let Some(unit) = self.workspace.unit_mut(project, branch) else {
            return;
        };
        let moved = match drop {
            Drop::Move { from, to } => {
                unit.move_tab(from, to);
                true
            }
            Drop::Join { from, onto, after } => unit.join_tabs(from, onto, after),
        };
        if moved {
            let shown = unit.active_tab;
            self.workspace.select_tab(project, branch, shown);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests;

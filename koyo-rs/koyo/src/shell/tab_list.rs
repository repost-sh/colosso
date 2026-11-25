//! A checkout's tabs as a list, along the top rail or down the sidebar as **Tabs** says: dragged,
//! a tab moves among them or joins another side by side, and pulled off the list it goes onto a
//! side of a pane; a pane of a split dropped on a tab goes where that tab is.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Along as _, AnyElement, AppContext as _, Axis, ClickEvent, Context, Div,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Point,
    Stateful, StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::Shell;
use super::reorder::{Drop, List, Reorder, TabHand};
use super::splits::{Dragged, DraggedPane};
use super::tab_face::{Face, split_chips};
use crate::model::Tab;
use crate::pane::ROW_BUTTON;
use crate::theme::Palette;

/// How much of the tab in hand shows while it's over a tab it would join.
const JOINING_OPACITY: f32 = 0.35;
/// The room a title leaves at its row's end for what shows there: a row's button, and a
/// little space before it.
const END: f32 = ROW_BUTTON + 6.;

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
    let list = (List::Checkout(at.0, at.1), axis);
    Reorder::new(list, tabs, hand.clone(), gap, p.accent, Rc::new(on_drop))
}

/// The tab `tab` at `at`, as `(project, branch, tab)`, in its list along `axis`, keyed by
/// what it keeps as it moves: `row` showing a split's panes as chips and `split_end` after them,
/// and a single pane as `single` has it. Dragged, it's taken up by the list, and what's dropped on it from a
/// split goes where it is.
pub fn tab_row(
    row: Stateful<Div>,
    (at, tab): ((usize, usize, usize), &Tab),
    (axis, hand): (Axis, &Rc<TabHand>),
    split_end: Option<Div>,
    p: Palette,
    cx: &mut Context<Shell>,
    single: impl FnOnce(Stateful<Div>, &Face, &mut Context<Shell>) -> Stateful<Div>,
) -> (u64, AnyElement) {
    let faces = Face::all(tab, cx);
    let row = match (tab, faces.first()) {
        (Tab::Single(_), Some(face)) => single(row, face, cx),
        _ => row
            .when(axis == Axis::Horizontal, |row| row.px(px(3.)))
            .child(split_chips(&faces, axis, p, cx))
            .children(split_end),
    };
    let key = tab.first().id;
    let row = tab_drag(row, (at, key), (axis, hand), faces, p, cx);
    (key, row.into_any_element())
}

/// `title`, and `end` over its end rather than after it, filling the rest of a sidebar row
/// (`docs/sidebar.md` 5): the title runs on to the end while nothing's there, and makes
/// room while `shown`, or while the pointer's on `group`, as `end` shows then. The title has an
/// id because GPUI lays a hover out only for an element that has one.
pub fn title_with_end(
    title: Stateful<Div>,
    end: impl IntoElement,
    shown: bool,
    group: &'static str,
) -> Div {
    let (title, room) = (title.flex_1().min_w_0(), px(END));
    let title = if shown {
        title.mr(room)
    } else {
        title.group_hover(group, move |style| style.mr(room))
    };
    let end = (div().absolute().top_0().bottom_0().right_0())
        .flex()
        .items_center()
        .child(end);
    (div().relative().flex_1().min_w_0().h_full())
        .flex()
        .items_center()
        .child(title)
        .child(end)
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
    let list = List::Checkout(at.0, at.1);
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
        .when(lifted, |row| row.bg(p.hover).shadow_sm())
        .when(joining, |row| row.opacity(JOINING_OPACITY))
        .on_drag(held, move |held, grab, _, cx| {
            hand.take(list, key, at.2, grab.along(axis));
            let chip = held.chip(grab);
            cx.new(|_| chip)
        })
        .drag_over::<DraggedPane>(move |style, held, _, _| match held.what {
            Dragged::Pane(_) | Dragged::Loose => style.bg(p.hover),
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
    /// `row`, the row of the tab at `at`, as `(project, branch, tab)`, on the rail or in the
    /// sidebar: picked, its tab shows, and double-clicked, the files it previews stay open
    /// (`keep_tab`); a middle click closes it.
    pub(super) fn tab_press(
        row: Stateful<Div>,
        at: (usize, usize, usize),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (project, branch, tab) = at;
        row.on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            this.pick_tab(project, branch, tab, cx);
            if event.click_count() > 1 {
                this.keep_tab(at, cx);
            }
        }))
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, _, window, cx| this.close_tab_at(at, window, cx)),
        )
    }

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

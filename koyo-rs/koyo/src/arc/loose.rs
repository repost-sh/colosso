//! The loose pages of the space shown (`docs/arc-sidebar.md` 11): the pages open in it that are
//! no project's, as rows under the pinned pages and over the open project, as Arc lists a
//! space's tabs. The one shown is a pill, as a worktree's tab is. A row is dragged among them, onto
//! a worktree's tabs, where it's a tab of that worktree, or onto the pinned pages, where it's
//! pinned; and a page's tab from a worktree, or a pinned page's tile, let go on them is one of
//! them. With none, there's no list: a dotted strip takes one that's in hand.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Div, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Point, Stateful, StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::worktrees::{ROW_END, arc_row, pill_row, tab_icon};
use super::{DraggedPin, Look};
use crate::model::Loose;
use crate::pane::{ROW_GLYPH, row_sized, sized_close_button};
use crate::shell::{Dragged, DraggedPane, Drop, Face, Reorder, Shell, title_with_end};

/// The list the loose pages are dragged along: no checkout's, whose lists are known by their
/// project and worktree.
const LIST: u64 = u64::MAX;
/// Between the rows, as between a worktree's.
const GAP: f32 = 2.;
/// Under the last of them, before the open project's name.
const AFTER: f32 = 8.;

/// The loose pages of the space shown, in a list they're dragged along; with none, the strip
/// that takes one while something they'd take is in hand, and nothing otherwise. `taking` says
/// something is: a page's tab off a worktree's list, or a pinned page's tile away from the
/// others.
pub fn list(shell: &Shell, taking: bool, look: Look, cx: &mut Context<Shell>) -> Option<Div> {
    let pages = shell.workspace.loose();
    if pages.is_empty() {
        return taking.then(|| div().pb(px(AFTER)).child(strip(look, cx)));
    }
    let shown = shell.workspace.loose_shown();
    let rows = (pages.iter().enumerate())
        .map(|(at, page)| row(shell, (at, page), (shown == Some(at), taking), look, cx))
        .collect();
    let on_drop = cx.listener(|this, drop: &Drop, _, cx| {
        if let Drop::Move { from, to } = *drop {
            this.move_loose(from, to, cx);
        }
    });
    let hand = shell.tab_hand.clone();
    let list = Reorder::pages(LIST, rows, hand, px(GAP), Rc::new(on_drop));
    Some(div().flex_shrink_0().pb(px(AFTER)).child(list))
}

/// Has `row` take what's let go on it for the loose pages, at `place` among them: a page's tab
/// from a worktree's list, or a pinned page's tile, which is unpinned there. Lit under either
/// while `taking`.
fn takes_pages(
    row: Stateful<Div>,
    (place, taking): (usize, bool),
    look: Look,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    row.drag_over::<DraggedPane>(move |style, held, _, _| match held.what {
        Dragged::Tab(..) if taking => style.bg(look.row_hover),
        _ => style,
    })
    .on_drop(cx.listener(move |this, held: &DraggedPane, _, cx| {
        if let (Dragged::Tab(..), [face]) = (held.what, held.faces.as_slice()) {
            this.loosen_tab_of(face.id, place, cx);
        }
    }))
    .drag_over::<DraggedPin>(move |style, _, _, _| style.bg(look.row_hover))
    .on_drop(cx.listener(move |this, pin: &DraggedPin, _, cx| {
        this.loosen_pinned(pin.key, place, cx);
    }))
}

/// Where a page let go is the space's first loose one, while it has none: a dotted row saying
/// so.
fn strip(look: Look, cx: &mut Context<Shell>) -> Stateful<Div> {
    let strip = arc_row("arc-loose-strip")
        .debug_selector(|| "arc-loose-strip".to_owned())
        .justify_center()
        .border_1()
        .border_dashed()
        .border_color(look.label.opacity(0.6))
        .text_xs()
        .text_color(look.label)
        .child("Drop to keep it in this space");
    // The visual check lets a tab go where the strip is drawn.
    #[cfg(feature = "visual-check")]
    let strip = {
        use gpui_kit::base::ElementExt as _;
        strip.on_prepaint(crate::visual::noting("arc-loose-strip".into()))
    };
    takes_pages(strip, (0, true), look, cx)
}

/// The row of the loose page `page`, in place `at`: a pill when it's the tab shown (`active`),
/// tinted while hovered, its × at its end on hover, and always on the one shown.
fn row(
    shell: &Shell,
    (at, page): (usize, &Loose),
    (active, taking): (bool, bool),
    look: Look,
    cx: &mut Context<Shell>,
) -> (u64, AnyElement) {
    let p = look.p;
    let key = page.key();
    let face = Face::of(page.tab.first(), cx);
    let hand = shell.tab_hand.clone();
    let lifted = hand.holding() == Some(key);
    let held = DraggedPane {
        what: Dragged::Loose,
        faces: vec![face.clone()],
        hand: Some(hand.clone()),
        grab: Point::default(),
    };
    let close = row_sized(sized_close_button(("arc-loose-close", key), ROW_GLYPH, p))
        .debug_selector(move || format!("arc-loose-close-{at}"))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.close_apart(key, Shell::close_loose, window, cx);
        }))
        .when(!active, |close| {
            close
                .opacity(0.)
                .group_hover("arc-loose", |style| style.opacity(1.))
        });
    let title = div()
        .id(("arc-loose-title", key))
        .flex()
        .child(face.label());
    let row = arc_row(("arc-loose", key))
        .group("arc-loose")
        .debug_selector(move || format!("arc-loose-{at}"));
    let row = pill_row(row, (key, active), shell, look, cx)
        .pr(px(ROW_END))
        // In hand, it's lifted off the list, over the rows it passes.
        .when(lifted, |row| row.bg(p.hover).shadow_sm())
        .on_click(cx.listener(move |this, _, _, cx| this.pick_loose(key, cx)))
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, _, window, cx| {
                this.close_apart(key, Shell::close_loose, window, cx);
            }),
        )
        .on_drag(held, move |held, grab, _, cx| {
            hand.take(LIST, key, at, grab.y);
            let chip = held.chip(grab);
            cx.new(|_| chip)
        })
        .child(tab_icon(&face, p))
        .child(title_with_end(title, close, active, "arc-loose"));
    // The visual check takes a row up where it's drawn.
    #[cfg(feature = "visual-check")]
    let row = {
        use gpui_kit::base::ElementExt as _;
        row.on_prepaint(crate::visual::noting(format!("arc-loose-{at}")))
    };
    (
        key,
        takes_pages(row, (at, taking), look, cx).into_any_element(),
    )
}

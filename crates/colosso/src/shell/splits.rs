//! The shown tab's panes: a split's two sides with a divider between them, dragged to share
//! the room otherwise; over each pane of a split a header, dragged to move the pane, with a
//! button that takes it into a tab of its own and one that closes it; and, while a tab or
//! pane is in hand, the sides of each pane it can be dropped on, the one under the pointer
//! lit as far as it would reach.

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Div, DragMoveEvent, ElementId, EmptyView,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled, Window, deferred, div, px, relative,
};

use super::{Shell, elements::ellipsized, icon};
use crate::model::{Edge, Pane, SPLIT_MIN, Split, SplitAxis, Tab};
use crate::pane::{self, close_button, tab_button};
use crate::theme::{self, Palette};

/// What's in hand, to split a pane with or to put in the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dragged {
    /// A tab, as `(project, branch, tab)`.
    Tab(usize, usize, usize),
    /// A pane of a split.
    Pane(u64),
}

/// A tab or pane in hand, drawn under the pointer as its icon and title.
#[derive(Clone)]
pub struct DraggedPane {
    pub what: Dragged,
    pub title: SharedString,
    pub icon: IconName,
}

impl Render for DraggedPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .h(px(28.))
            .max_w(px(240.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(6.))
            .border_1()
            .border_color(p.hairline)
            .bg(p.raised)
            .shadow_md()
            .text_sm()
            .text_color(p.text)
            .child(icon(self.icon, 13.))
            .child(ellipsized(self.title.clone()))
    }
}

/// A split's divider in hand, by the split's path in its tab.
#[derive(Clone)]
pub struct SplitDivider(Vec<bool>);

/// The line between two sides, `row` when they sit side by side, picked up a few pixels either
/// side of it to drag `payload`. The part picked up is among the window's overlays, so a page
/// either side, in a native layer over the window, doesn't hide the part of it over the page.
pub fn divider<T: Clone + 'static>(
    id: impl Into<ElementId>,
    selector: SharedString,
    row: bool,
    payload: T,
    p: Palette,
) -> Div {
    let handle = div()
        .id(id)
        .debug_selector(move || selector.to_string())
        .absolute()
        .occlude()
        .map(|handle| {
            if row {
                handle
                    .top_0()
                    .bottom_0()
                    .left(px(-3.))
                    .w(px(7.))
                    .cursor_col_resize()
            } else {
                handle
                    .left_0()
                    .right_0()
                    .top(px(-3.))
                    .h(px(7.))
                    .cursor_row_resize()
            }
        })
        .on_drag(payload, |_, _, _, cx| cx.new(|_| EmptyView));
    let line = div().flex_shrink_0().relative().bg(p.hairline);
    if row {
        line.w(px(1.)).h_full().child(deferred(handle))
    } else {
        line.h(px(1.)).w_full().child(deferred(handle))
    }
}

/// The height of the header over each pane of a split.
const HEADER: f32 = 28.;

impl Shell {
    /// The shown tab: its pane, or its split.
    pub(super) fn tab_content(
        &self,
        tab: &Tab,
        context: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::palette(cx);
        let in_hand = cx.has_active_drag();
        self.part(tab, Vec::new(), context, p, in_hand, cx)
    }

    fn part(
        &self,
        tab: &Tab,
        path: Vec<bool>,
        context: &str,
        p: Palette,
        in_hand: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match tab {
            Tab::Single(pane) => {
                let in_split = !path.is_empty();
                self.pane_part(pane, in_split, context, p, in_hand, cx)
            }
            Tab::Split(split) => self.split_part(split, path, context, p, in_hand, cx),
        }
    }

    fn split_part(
        &self,
        split: &Split,
        path: Vec<bool>,
        context: &str,
        p: Palette,
        in_hand: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = split.axis == SplitAxis::Row;
        let ratio = split.ratio.clamp(SPLIT_MIN, 1. - SPLIT_MIN);
        let side = |path: &[bool], second: bool| {
            let mut path = path.to_vec();
            path.push(second);
            path
        };
        let first = self.part(&split.first, side(&path, false), context, p, in_hand, cx);
        let second = self.part(&split.second, side(&path, true), context, p, in_hand, cx);
        let id = SharedString::from(format!("split-{path:?}"));
        let dragged = path.clone();
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .when(!row, Styled::flex_col)
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<SplitDivider>, _, cx| {
                    if event.drag(cx).0 != dragged {
                        return;
                    }
                    let (bounds, at) = (event.bounds, event.event.position);
                    let share = if row {
                        (at.x - bounds.left()) / bounds.size.width
                    } else {
                        (at.y - bounds.top()) / bounds.size.height
                    };
                    this.resize_split(&dragged, share, cx);
                }),
            )
            .child(
                div()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .map(|side| {
                        if row {
                            side.h_full().w(relative(ratio))
                        } else {
                            side.w_full().h(relative(ratio))
                        }
                    })
                    .child(first),
            )
            .child(divider(id.clone(), id, row, SplitDivider(path), p))
            .child(div().flex_1().min_w_0().min_h_0().flex().child(second))
            .into_any_element()
    }

    /// The split at `path` in the shown tab gives its first side `share` of the room.
    fn resize_split(&mut self, path: &[bool], share: f32, cx: &mut Context<Self>) {
        let Some(unit) = self.workspace.branch_mut() else {
            return;
        };
        let at = unit.active_tab;
        let Some(split) = unit.tabs.get_mut(at).and_then(|tab| tab.split_at_mut(path)) else {
            return;
        };
        let share = share.clamp(SPLIT_MIN, 1. - SPLIT_MIN);
        if (split.ratio - share).abs() > f32::EPSILON {
            split.ratio = share;
            cx.notify();
        }
    }

    fn pane_part(
        &self,
        pane: &Pane,
        in_split: bool,
        context: &str,
        p: Palette,
        in_hand: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(in_split, |part| part.child(self.pane_header(pane, p, cx)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(pane::pane(pane, context, p)),
            )
            .when(in_hand, |part| {
                // Among the window's overlays, as a page's native layer hides what's under it.
                part.children(drop_zones(pane.id, p, cx).into_iter().map(deferred))
            })
            .into_any_element()
    }

    /// The header over a pane of a split: dragged, it moves the pane.
    fn pane_header(&self, pane: &Pane, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let id = pane.id;
        let held = DraggedPane {
            what: Dragged::Pane(id),
            title: pane.title.clone(),
            icon: pane.kind.icon(),
        };
        let button_press =
            |_: &gpui_kit::MouseDownEvent, _: &mut Window, cx: &mut gpui_kit::App| {
                cx.stop_propagation();
            };
        div()
            .id(("pane-header", id))
            .h(px(HEADER))
            .flex_shrink_0()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(p.hairline)
            .text_xs()
            .text_color(p.muted)
            .cursor_grab()
            .on_drag(held, |held, _, _, cx| {
                let held = held.clone();
                cx.new(|_| held)
            })
            .child(icon(IconName::GripVertical, 12.))
            .child(icon(pane.kind.icon(), 12.))
            .child(ellipsized(pane.title.clone()).flex_1())
            .child(
                tab_button(("pane-own-tab", id), p)
                    .debug_selector(move || format!("pane-own-tab-{id}"))
                    .tooltip(|window, cx| {
                        Tooltip::new("Open in a tab of its own").build(window, cx)
                    })
                    .child(icon(IconName::SquareArrowOutUpRight, 12.))
                    .on_mouse_down(MouseButton::Left, button_press)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.pane_to_own_tab(id, cx);
                    })),
            )
            .child(
                close_button(("pane-close", id), p)
                    .debug_selector(move || format!("pane-close-{id}"))
                    .on_mouse_down(MouseButton::Left, button_press)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.close_pane(id, cx);
                    })),
            )
    }

    /// Takes pane `id` out of its split into a tab right after the one shown.
    fn pane_to_own_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(unit) = self.workspace.branch_mut() {
            let after = unit.active_tab + 1;
            if unit.pane_to_tab(id, after) {
                cx.notify();
            }
        }
    }

    /// `held` dropped on `edge` of pane `target`.
    fn drop_on_pane(&mut self, held: Dragged, target: u64, edge: Edge, cx: &mut Context<Self>) {
        let selected = self.selected_branch();
        let Some(unit) = self.workspace.branch_mut() else {
            return;
        };
        let moved = match held {
            Dragged::Tab(project, branch, at) if Some((project, branch)) == selected => {
                unit.drop_tab(at, target, edge)
            }
            Dragged::Tab(..) => false,
            Dragged::Pane(id) => unit.move_pane(id, target, edge),
        };
        if moved {
            cx.notify();
        }
    }

    /// `held` dropped on the tab at `at` in the strip: it goes where that tab is.
    pub(super) fn drop_on_strip(
        &mut self,
        held: Dragged,
        at: (usize, usize, usize),
        cx: &mut Context<Self>,
    ) {
        let (project, branch, to) = at;
        if self.selected_branch() != Some((project, branch)) {
            return;
        }
        let Some(unit) = self.workspace.branch_mut() else {
            return;
        };
        match held {
            Dragged::Tab(from_project, from_branch, from)
                if (from_project, from_branch) == (project, branch) =>
            {
                unit.move_tab(from, to);
            }
            Dragged::Tab(..) => return,
            Dragged::Pane(id) => {
                unit.pane_to_tab(id, to);
            }
        }
        cx.notify();
    }

    /// The selected checkout, as `(project, branch)`.
    fn selected_branch(&self) -> Option<(usize, usize)> {
        let project = self.workspace.project()?;
        Some((self.workspace.active_project, project.active_branch))
    }
}

/// A tab in the strip, `at` as `(project, branch, tab)`: dragged, it goes onto a pane's side
/// or elsewhere in the strip; what's dropped on it goes where it is.
pub fn tab_drag(
    row: Stateful<Div>,
    at: (usize, usize, usize),
    tab: &Tab,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let held = DraggedPane {
        what: Dragged::Tab(at.0, at.1, at.2),
        title: tab.title(),
        icon: tab.icon(),
    };
    row.debug_selector(move || format!("tab-{}-{}-{}", at.0, at.1, at.2))
        .on_drag(held, |held, _, _, cx| {
            let held = held.clone();
            cx.new(|_| held)
        })
        .drag_over::<DraggedPane>(move |style, _, _, _| style.bg(p.hover))
        .on_drop(cx.listener(move |this, held: &DraggedPane, _, cx| {
            this.drop_on_strip(held.what, at, cx);
        }))
}

/// Over a pane while something's in hand: a strip along each side to drop it on, and under
/// them how far it would reach, lit while it's over that side.
fn drop_zones(target: u64, p: Palette, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let lit = p.accent.opacity(0.16);
    let line = p.accent.opacity(0.6);
    // Drawn again on every move while something's in hand, so each side's name is made once.
    let groups = Edge::ALL.map(|edge| SharedString::from(format!("drop-{target}-{edge:?}")));
    let mut parts = Vec::new();
    for (edge, group) in Edge::ALL.into_iter().zip(groups.clone()) {
        // Where it would go: the half of the pane on that side.
        let reach = div().absolute().map(|reach| match edge {
            Edge::Left => reach.left_0().top_0().bottom_0().w(relative(0.5)),
            Edge::Right => reach.right_0().top_0().bottom_0().w(relative(0.5)),
            Edge::Top => reach.left_0().right_0().top_0().h(relative(0.5)),
            Edge::Bottom => reach.left_0().right_0().bottom_0().h(relative(0.5)),
        });
        parts.push(
            reach
                .rounded(px(6.))
                .group_drag_over::<DraggedPane>(group, move |style| {
                    style.bg(lit).border_2().border_color(line)
                })
                .into_any_element(),
        );
    }
    for (edge, group) in Edge::ALL.into_iter().zip(groups) {
        // Where it's dropped: a quarter along the sides, the middle split top and bottom.
        let zone = div().absolute().map(|zone| match edge {
            Edge::Left => zone.left_0().top_0().bottom_0().w(relative(0.25)),
            Edge::Right => zone.right_0().top_0().bottom_0().w(relative(0.25)),
            Edge::Top => zone
                .left(relative(0.25))
                .right(relative(0.25))
                .top_0()
                .h(relative(0.5)),
            Edge::Bottom => zone
                .left(relative(0.25))
                .right(relative(0.25))
                .bottom_0()
                .h(relative(0.5)),
        });
        parts.push(
            zone.id(group.clone())
                .debug_selector({
                    let group = group.clone();
                    move || group.to_string()
                })
                .group(group)
                .on_drop(cx.listener(move |this, held: &DraggedPane, _, cx| {
                    this.drop_on_pane(held.what, target, edge, cx);
                }))
                .into_any_element(),
        );
    }
    parts
}

#[cfg(test)]
mod tests;

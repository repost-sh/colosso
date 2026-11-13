//! The shown tab's panes: a split's two sides with a divider between them, dragged to share
//! the room otherwise; over each pane of a split a header, dragged to move the pane, with a
//! button that takes it into a tab of its own and one that closes it; and, while a tab or
//! pane is in hand, the sides of each pane it can be dropped on, the one under the pointer
//! lit as far as it would reach.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Bounds, Context, DispatchPhase, DragMoveEvent,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Point, Render, SharedString, StatefulInteractiveElement as _, Styled, Window, canvas, deferred,
    div, point, px, relative,
};

use super::elements::pane_close_button;
use super::reorder::TabHand;
use super::tab_face::Face;
use super::{Shell, icon};
use crate::model::{Edge, Pane, SPLIT_MIN, Split, SplitAxis, Tab};
use crate::pane::{CLOSE, tab_button};
use crate::theme::card::part;
use crate::theme::{self, Palette};

/// What's in hand, to split a pane with or to put in the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dragged {
    /// A tab, as `(project, branch, tab)`.
    Tab(usize, usize, usize),
    /// A pane of a split.
    Pane(u64),
}

/// Where the chip sits from the pointer: just up and left of it, the pointer over its icon.
const CHIP_AT: Point<Pixels> = point(px(12.), px(14.));

/// A tab or pane in hand, drawn under the pointer as its panes' icons and titles: a tab
/// only once it's off its list, as on it the tab itself is drawn under the pointer.
#[derive(Clone)]
pub struct DraggedPane {
    pub what: Dragged,
    pub faces: Vec<Face>,
    /// For a tab, what says whether it's off its list.
    pub hand: Option<Rc<TabHand>>,
    /// Where in what was taken up the pointer took hold, so the chip is drawn by the pointer
    /// rather than as far into it.
    pub grab: Point<Pixels>,
}

impl DraggedPane {
    /// The chip under the pointer, for what was taken up `grab` in from its corner.
    pub fn chip(&self, grab: Point<Pixels>) -> Self {
        DraggedPane {
            grab,
            ..self.clone()
        }
    }
}

impl Render for DraggedPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let chip = div();
        if self.hand.as_ref().is_some_and(|hand| !hand.chip_shows()) {
            return chip;
        }
        // Drawn from where the pointer is less how far in it took hold, so set back by that.
        let by = (self.grab - CHIP_AT).max(&Point::default());
        // A split's panes each in a chip of their own, as its tab shows them.
        let split = self.faces.len() > 1;
        let faces = self.faces.iter().map(|face| {
            (face.chip().max_w(px(200.)).px_1p5()).when(split, |chip| chip.bg(p.hover))
        });
        chip.pl(by.x).pt(by.y).child(
            div()
                .h(px(28.))
                .px_1()
                .flex()
                .items_center()
                .gap_1()
                .rounded(px(6.))
                .border_1()
                .border_color(p.hairline)
                .bg(p.raised)
                .shadow_md()
                .text_sm()
                .text_color(p.text)
                .children(faces),
        )
    }
}

/// A split's divider in hand, by the split's path in its tab.
#[derive(Clone)]
pub struct SplitDivider(Vec<bool>);

/// The height of the header over each pane of a split.
const HEADER: f32 = 28.;

impl Shell {
    /// The shown tab: its pane, or its split.
    pub(super) fn tab_content(&self, tab: &Tab, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let in_hand = cx.has_active_drag();
        self.part(tab, Vec::new(), p, in_hand, cx)
    }

    fn part(
        &self,
        tab: &Tab,
        path: Vec<bool>,
        p: Palette,
        in_hand: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match tab {
            Tab::Single(pane) => {
                let in_split = !path.is_empty();
                self.pane_part(pane, in_split, p, in_hand, cx)
            }
            Tab::Split(split) => self.split_part(split, path, p, in_hand, cx),
        }
    }

    fn split_part(
        &self,
        split: &Split,
        path: Vec<bool>,
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
        let first = self.part(&split.first, side(&path, false), p, in_hand, cx);
        let second = self.part(&split.second, side(&path, true), p, in_hand, cx);
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
            .child(theme::card::seam(
                id,
                row,
                SplitDivider(path),
                p,
                theme::card::islands(cx),
            ))
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
        p: Palette,
        in_hand: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, bar) = (pane.id, pane.bar());
        // In Arc an island of its own, a split's sides the glass apart.
        part(p, theme::card::islands(cx))
            .debug_selector(move || format!("pane-{id}"))
            .size_full()
            .flex()
            .flex_col()
            .child(hears_presses(id, cx))
            // A pane whose content has a bar (a device's standing and buttons) has its header
            // alone too.
            .when(in_split || bar.is_some(), |part| part.child(self.pane_header(pane, bar, in_split, p, cx)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.pane_content(pane, p, cx)),
            )
            .when(in_hand, |part| {
                // Among the window's overlays, over the pane and whatever's drawn after it.
                part.children(drop_zones(pane.id, p, cx).into_iter().map(deferred))
            })
            .into_any_element()
    }

    /// The header over a pane: of a split, dragged, it moves the pane, and has the pane's own
    /// buttons; one whose content has a bar (a device's) has it after its name, in a split or
    /// alone.
    fn pane_header(&self, pane: &Pane, bar: Option<AnyView>, in_split: bool, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, face) = (pane.id, Face::of(pane, cx));
        let (icon_of_pane, label, dirty) = (face.icon(12.), face.label(), face.dirty);
        let held = DraggedPane {
            what: Dragged::Pane(id),
            faces: vec![face],
            hand: None,
            grab: Point::default(),
        };
        let button_press = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
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
            .when(in_split, |header| {
                let dragged = header.cursor_grab().on_drag(held, |held, grab, _, cx| {
                    let chip = held.chip(grab);
                    cx.new(|_| chip)
                });
                dragged.child(icon(IconName::GripVertical, 12.))
            })
            .child(icon_of_pane)
            .map(|header| match bar {
                // The room left is given here, by what's laid out with every frame: a view kept
                // from one frame to the next fills it, and isn't the one to stretch into it.
                Some(bar) => header.child(label.flex_shrink_0()).child(div().flex_1().min_w_0().child(bar)),
                None => header.child(label.flex_1()),
            })
            .when(in_split, |header| {
                let own_tab = tab_button(("pane-own-tab", id), p)
                    .debug_selector(move || format!("pane-own-tab-{id}"))
                    .tooltip(|window, cx| Tooltip::new("Open in a tab of its own").build(window, cx))
                    .child(icon(IconName::SquareArrowOutUpRight, 12.))
                    .on_mouse_down(MouseButton::Left, button_press)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.pane_to_own_tab(id, cx);
                    }));
                let close = pane_close_button("pane-close", id, (dirty, CLOSE), p, cx);
                header.child(own_tab).child(close)
            })
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

    /// `held` dropped on the tab at `at` in the strip: a pane of a split goes where that tab
    /// is, and so does a tab from another checkout's list. A tab of this list is the list's to
    /// place, as it's let go.
    pub(super) fn drop_on_strip(
        &mut self,
        held: &DraggedPane,
        at: (usize, usize, usize),
        cx: &mut Context<Self>,
    ) {
        let (project, branch, to) = at;
        match held.what {
            Dragged::Pane(id) => {
                if let Some(unit) = self.workspace.unit_mut(project, branch)
                    && unit.pane_to_tab(id, to)
                {
                    cx.notify();
                }
            }
            // From another checkout's list: it goes where the tab it's dropped on is.
            Dragged::Tab(..) => self.drop_tab_on_checkout(held, (project, branch), to, cx),
        }
    }

    /// A tab from another checkout's list in the Arc sidebar, let go over the checkout `to`,
    /// as `(project, branch)`: it moves there, at `at` among its tabs, and shows
    /// (`docs/arc-sidebar.md` 5). It's found by its pane, as its place may have changed since it
    /// was taken up; one from the checkout itself is its list's to place.
    pub(crate) fn drop_tab_on_checkout(
        &mut self,
        held: &DraggedPane,
        to: (usize, usize),
        at: usize,
        cx: &mut Context<Self>,
    ) {
        let (Dragged::Tab(..), Some(face)) = (held.what, held.faces.first()) else {
            return;
        };
        let Some((from, _)) = self.workspace.find_pane(|pane| pane.id == face.id) else {
            return;
        };
        if self.workspace.move_tab_to_checkout(from, to, at) {
            cx.notify();
        }
    }

    /// The selected checkout, as `(project, branch)`.
    fn selected_branch(&self) -> Option<(usize, usize)> {
        let project = self.workspace.project()?;
        Some((self.workspace.active_project()?, project.active_branch))
    }
}

/// Hears a press anywhere over pane `id`, which puts the person in that pane: on its way down,
/// and by where the pane is rather than by what's under the pointer, as much of what a pane
/// shows keeps presses from what's behind it (a chat's composer, a page's notice).
fn hears_presses(id: u64, cx: &mut Context<Shell>) -> impl IntoElement {
    let shell = cx.weak_entity();
    let hear = move |bounds: Bounds<Pixels>, (): (), window: &mut Window, _: &mut App| {
        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && bounds.contains(&event.position) {
                (shell.update(cx, |shell, cx| shell.enter_pane(id, cx))).ok();
            }
        });
    };
    canvas(|_, _, _| (), hear)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// Over a pane while something's in hand: a strip along each side to drop it on, and under
/// them how far it would reach, lit while it's over that side.
fn drop_zones(target: u64, p: Palette, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let lit = p.accent.opacity(0.16);
    let line = p.accent.opacity(0.6);
    let zones = Edge::ALL.into_iter().map(|edge| {
        let group = SharedString::from(format!("drop-{target}-{edge:?}"));
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
        // Where it would go, the half of the pane on that side, twice the zone's width. It's
        // drawn from inside the zone, as only what's in a group finds the group.
        let reach = div()
            .absolute()
            .top_0()
            .h_full()
            .w(relative(2.))
            .map(|reach| match edge {
                Edge::Left => reach.left_0(),
                Edge::Right => reach.right_0(),
                Edge::Top | Edge::Bottom => reach.left(relative(-0.5)),
            })
            .rounded(px(6.))
            .group_drag_over::<DraggedPane>(group.clone(), move |style| {
                style.bg(lit).border_2().border_color(line)
            })
            // gpui styles only what has a hitbox, which a drag-over style gives it; one that
            // doesn't block the zone under it, which still takes the drop.
            .drag_over::<DraggedPane>(|style, _, _, _| style);
        zone.id(group.clone())
            .debug_selector({
                let group = group.clone();
                move || group.to_string()
            })
            .group(group)
            .child(reach)
            .on_drop(cx.listener(move |this, held: &DraggedPane, _, cx| {
                this.drop_on_pane(held.what, target, edge, cx);
            }))
            .into_any_element()
    });
    zones.collect()
}

#[cfg(test)]
mod tests;

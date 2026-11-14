//! What a menu's window shows: the surface every menu has, and the rows most of them hold.

use gpui_kit::component::kbd::Kbd;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyView, AnyWindowHandle, App, Context, Div, ElementId, FocusHandle,
    InteractiveElement as _, IntoElement, MouseMoveEvent, ParentElement as _, Pixels, Render, Size,
    Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px,
};

use super::switch::row_switch;
use super::{
    ICON, LABEL_LINE, LABEL_SIZE, MenuBody, MenuItem, PADDING, PAGE, ROW_GAP, ROW_PADDING,
    SEPARATOR_MARGIN, SHORTCUT, close, separated,
};
use crate::list;
use crate::shell::icon;
use crate::theme::{self, Palette};

pub(super) struct Menu {
    pub(super) body: AnyView,
    /// What the window is filled with instead of a menu's glass (`MenuBody::backdrop`).
    pub(super) backdrop: Option<gpui_kit::Hsla>,
    /// A callout's arrow, by how far along the window's top it points (`MenuBody::callout`),
    /// and the size the window's glass was last cut to that shape at.
    pub(super) tip: Option<Pixels>,
    pub(super) shaped: Option<Size<Pixels>>,
    pub(super) _keys: Subscription,
    pub(super) _fits: Subscription,
}

impl Render for Menu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let fill = self.backdrop.unwrap_or(p.menu);
        let Some(tip) = self.tip else {
            return div()
                .size_full()
                .p(px(PADDING))
                .flex()
                .flex_col()
                .bg(fill)
                .text_color(p.text)
                .child(self.body.clone())
                .into_any_element();
        };
        // The window's blur is cut to the callout's shape, again whenever the window's size
        // changes; what's drawn over it is that shape too, the arrow and then the rest.
        let extent = window.viewport_size();
        if self.shaped.replace(extent) != Some(extent) {
            let (high, half, round) = (super::TIP, super::TIP_HALF, super::CALLOUT_RADIUS);
            crate::native::window::callout(window, tip.as_f32(), high, half, round, cx);
        }
        // The frame a floating window has, which macOS draws only round a rectangle: a dark
        // hairline at the very edge and a light one just inside it.
        let dark = p.text.l > 0.5;
        let (outer, inner) = if dark {
            (theme::gray(0., 0.45), theme::gray(1., 0.16))
        } else {
            (theme::gray(0., 0.2), theme::gray(1., 0.7))
        };
        // One outline for the callout, arrow and all: filled with its glass, then framed.
        let shape = gpui_kit::canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let mut filled = gpui_kit::PathBuilder::fill();
                outline(&mut filled, bounds, tip, 0.5);
                if let Ok(path) = filled.build() {
                    window.paint_path(path, fill);
                }
                for (inset, colour) in [(1.5, inner), (0.5, outer)] {
                    let mut edge = gpui_kit::PathBuilder::stroke(px(1.));
                    outline(&mut edge, bounds, tip, inset);
                    if let Ok(path) = edge.build() {
                        window.paint_path(path, colour);
                    }
                }
            },
        )
        .absolute()
        .size_full();
        // The surface is the callout's body, under the arrow; the shape behind it has its fill.
        let body = div()
            .absolute()
            .top(px(super::TIP))
            .left_0()
            .right_0()
            .bottom_0()
            .p(px(PADDING))
            .flex()
            .flex_col()
            .text_color(p.text)
            .child(self.body.clone());
        div().size_full().relative().child(shape).child(body).into_any_element()
    }
}

/// A callout's outline `inset` points within `bounds`, clockwise from its top left: the top
/// edge with the arrow standing on it, its point `tip` along, and four round corners, each a
/// few short lines. Half a point in, a line a point wide lies within the window.
fn outline(
    path: &mut gpui_kit::PathBuilder,
    bounds: gpui_kit::Bounds<Pixels>,
    tip: Pixels,
    inset: f32,
) {
    use gpui_kit::point;
    let inset = px(inset);
    let (left, right) = (bounds.left() + inset, bounds.right() - inset);
    let (top, bottom) = (bounds.top() + px(super::TIP) + inset - px(0.5), bounds.bottom() - inset);
    let round = px(super::CALLOUT_RADIUS) - inset + px(0.5);
    let half = px(super::TIP_HALF) - inset + px(0.5);
    // The arrow stays on the straight of the top edge.
    let middle = (bounds.left() + tip).clamp(left + round + half, right - round - half);
    path.move_to(point(left + round, top));
    path.line_to(point(middle - half, top));
    path.line_to(point(middle, bounds.top() + inset));
    path.line_to(point(middle + half, top));
    let corners = [
        (right - round, top + round, 270.),
        (right - round, bottom - round, 0.),
        (left + round, bottom - round, 90.),
        (left + round, top + round, 180.),
    ];
    for (x, y, from) in corners {
        for step in 0..=8 {
            let turn = (from + step as f32 * 11.25_f32).to_radians();
            path.line_to(point(x + round * turn.cos(), y + round * turn.sin()));
        }
    }
    path.close();
}

/// A menu of rows, each saying its action to a focus handle in the window that opened the menu.
pub(super) struct Rows {
    pub(super) items: Vec<(MenuItem, Option<Kbd>)>,
    /// What the rows take, measured once: their labels don't change while the menu's up.
    pub(super) extent: Size<Pixels>,
    /// The row the keys and the pointer are at.
    pub(super) highlighted: Option<usize>,
    pub(super) target: FocusHandle,
    pub(super) parent: AnyWindowHandle,
}

/// A row of a menu, all that makes it look like one.
pub(crate) fn row(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .min_h(px(theme::ROW_HEIGHT))
        .flex_shrink_0()
        .px(px(ROW_PADDING))
        .flex()
        .items_center()
        .gap(px(ROW_GAP))
        .rounded(px(6.))
        .text_size(LABEL_SIZE)
        .cursor_pointer()
}

impl MenuBody for Rows {
    fn extent(&self, _: &Window, _: &App) -> Size<Pixels> {
        self.extent
    }

    fn hears(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "up" => self.step(-1, cx),
            "down" => self.step(1, cx),
            "pageup" => self.step(-PAGE, cx),
            "pagedown" => self.step(PAGE, cx),
            "enter" => match self.highlighted {
                Some(at) => self.pick(at, cx),
                None => return false,
            },
            _ => return false,
        }
        true
    }
}

impl Rows {
    /// `by` rows up or down, as far as there are any.
    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        self.highlight(list::stepped(self.highlighted, by, self.items.len()), cx);
    }

    fn highlight(&mut self, row: Option<usize>, cx: &mut Context<Self>) {
        if self.highlighted != row {
            self.highlighted = row;
            cx.notify();
        }
    }

    /// Has row `index` say what it says, once neither window is taken up by what got here: a press
    /// has the menu's, a key the one that has the keys, or the menu's. That's the end of the menu,
    /// unless it's a switch, which flips where it is.
    pub(super) fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((item, _)) = self.items.get_mut(index) else {
            return;
        };
        let action = item.action.boxed_clone();
        let stays = if let Some(on) = &mut item.switch {
            *on = !*on;
            cx.notify();
            true
        } else {
            false
        };
        let (target, parent) = (self.target.clone(), self.parent);
        cx.defer(move |cx| {
            if !stays {
                close(cx);
            }
            parent
                .update(cx, |_, window, cx| {
                    target.dispatch_action(&*action, window, cx);
                })
                .ok();
        });
    }

    fn row(
        index: usize,
        (item, shortcut): &(MenuItem, Option<Kbd>),
        switch: Option<AnyElement>,
        highlighted: bool,
        p: Palette,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let keys = || {
            shortcut
                .clone()
                .map(|keys| keys.appearance(false).into_any_element())
        };
        row(index)
            .when(highlighted, |row| row.bg(p.selected))
            // Moved, as it may rest on a row while the keys go elsewhere.
            .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                this.highlight(Some(index), cx);
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if !*hovered && this.highlighted == Some(index) {
                    this.highlight(None, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.pick(index, cx)))
            .child(match item.icon {
                Some(mark) => icon(mark, ICON).text_color(p.muted).into_any_element(),
                None => div().size(px(ICON)).flex_shrink_0().into_any_element(),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .line_height(px(LABEL_LINE))
                    .child(item.label.clone()),
            )
            .when_some(switch.or_else(keys), |row, trailing| {
                row.child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .justify_end()
                        .w(px(SHORTCUT))
                        .text_xs()
                        .text_color(p.muted)
                        .child(trailing),
                )
            })
    }
}

impl Render for Rows {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let switches: Vec<_> = (self.items.iter().enumerate())
            .map(|(index, (item, _))| item.switch.map(|on| row_switch(index, on, window, cx)))
            .collect();
        let rows = self
            .items
            .iter()
            .zip(switches)
            .enumerate()
            .map(|(index, (row, switch))| {
                let line = separated(&self.items, index).then(|| {
                    div()
                        .h(px(1.))
                        .mx(px(ROW_PADDING))
                        .my(px(SEPARATOR_MARGIN))
                        .bg(p.hairline)
                });
                let highlighted = self.highlighted == Some(index);
                div().flex_shrink_0().children(line).child(Self::row(
                    index,
                    row,
                    switch,
                    highlighted,
                    p,
                    cx,
                ))
            });
        div().size_full().flex().flex_col().children(rows)
    }
}

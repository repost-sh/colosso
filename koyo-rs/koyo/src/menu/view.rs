//! What a menu's window shows: the surface every menu has, and the rows most of them hold.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::{
    AnyElement, AnyView, AnyWindowHandle, Context, Div, FocusHandle, InteractiveElement as _, IntoElement,
    MouseMoveEvent, ParentElement as _, Render, Stateful, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px,
};

use super::{
    ICON, LABEL_LINE, LABEL_SIZE, MenuBody, MenuItem, PADDING, PAGE, ROW_GAP, ROW_PADDING,
    SEPARATOR_MARGIN, SHORTCUT, close, separated,
};
use super::switch::row_switch;
use crate::list;
use crate::shell::icon;
use crate::theme::{self, Palette};

pub(super) struct Menu {
    pub(super) body: AnyView,
    pub(super) _subscriptions: Vec<Subscription>,
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .size_full()
            .p(px(PADDING))
            .flex()
            .flex_col()
            .bg(p.menu)
            .text_color(p.text)
            .child(self.body.clone())
    }
}

/// A menu of rows, each saying its action to a focus handle in the window that opened the menu.
pub(super) struct Rows {
    pub(super) items: Vec<(MenuItem, Option<Kbd>)>,
    /// The row the keys and the pointer are at.
    pub(super) highlighted: Option<usize>,
    pub(super) target: FocusHandle,
    pub(super) parent: AnyWindowHandle,
}

impl MenuBody for Rows {
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
                .update(cx, |_, window, cx| target.dispatch_action(&*action, window, cx))
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
        let keys = || shortcut.clone().map(|keys| keys.appearance(false).into_any_element());
        div()
            .id(index)
            .min_h(px(theme::ROW_HEIGHT))
            .flex_shrink_0()
            .px(px(ROW_PADDING))
            .flex()
            .items_center()
            .gap(px(ROW_GAP))
            .rounded(px(6.))
            .text_size(LABEL_SIZE)
            .cursor_pointer()
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
        let rows = self.items.iter().zip(switches).enumerate().map(|(index, (row, switch))| {
            let line = separated(&self.items, index).then(|| {
                div().h(px(1.)).mx(px(ROW_PADDING)).my(px(SEPARATOR_MARGIN)).bg(p.hairline)
            });
            let highlighted = self.highlighted == Some(index);
            div()
                .flex_shrink_0()
                .children(line)
                .child(Self::row(index, row, switch, highlighted, p, cx))
        });
        div().size_full().flex().flex_col().children(rows)
    }
}

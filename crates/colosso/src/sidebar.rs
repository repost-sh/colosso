//! The sidebar's frame, shared by the workspace sidebars and the settings sidebar: its width,
//! the chrome the layout gives it, the grab strip that resizes it, and its slide in and out.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Empty, InteractiveElement as _, IntoElement,
    ParentElement, Render, RenderOnce, StatefulInteractiveElement as _, Styled as _, Window, div,
    px,
};

use crate::settings::Layout;
use crate::theme;

/// A sidebar of the given width with the chrome its layout puts around it: inside the card
/// with a hairline in Orca, on the glass in Arc.
#[derive(IntoElement)]
pub struct SidebarFrame {
    layout: Layout,
    width: f32,
    children: Vec<AnyElement>,
}

impl SidebarFrame {
    pub fn new(layout: Layout, width: f32) -> Self {
        SidebarFrame {
            layout,
            width,
            children: Vec::new(),
        }
    }
}

impl ParentElement for SidebarFrame {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SidebarFrame {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .relative()
            .w(px(self.width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .map(|frame| match self.layout {
                Layout::Orca => frame.p_2().border_r_1().border_color(p.hairline),
                Layout::Arc => frame.pr(px(theme::GUTTER)).pb(px(theme::GUTTER)),
            })
            .children(self.children)
            .child(resize_handle())
    }
}

/// The drag payload while the sidebar's right border is being dragged.
pub struct SidebarResize;

impl Render for SidebarResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// A grab strip along the sidebar's right border.
fn resize_handle() -> impl IntoElement {
    div()
        .id("sidebar-resize")
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(6.))
        .cursor_col_resize()
        .on_drag(SidebarResize, |_, _, _, cx| cx.new(|_| SidebarResize))
}

/// Shows the revealed part of a `width`-wide sidebar, sliding it out from under the left edge
/// so its contents keep their layout while it moves.
pub fn slide_out(sidebar: impl IntoElement, width: f32, reveal: f32) -> impl IntoElement {
    div()
        .w(px(width * reveal))
        .flex_shrink_0()
        .h_full()
        .flex()
        .overflow_hidden()
        .child(
            div()
                .flex_shrink_0()
                .h_full()
                .relative()
                .left(px(width * (reveal - 1.)))
                .child(sidebar),
        )
}

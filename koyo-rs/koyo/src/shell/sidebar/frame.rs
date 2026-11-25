//! The sidebar's frame, on the window's glass: its width, the grab strip that
//! resizes it (`elements::resize_handle`), and its slide in and out. Settings' own pages are the
//! Settings tab's content, on an island, not in this frame (`settings_view/page_list.rs`).
//! It has no fill of its own: it's on the glass the top rail is on, one with it.

use gpui_kit::{
    AnyElement, App, Context, Empty, IntoElement, ParentElement,
    Render, RenderOnce, Styled as _, Window, div, px,
};

use crate::shell::resize_handle;
use crate::theme;

/// A sidebar of the given width on the glass, a gutter's margin before the islands.
#[derive(IntoElement)]
pub struct SidebarFrame {
    width: f32,
    children: Vec<AnyElement>,
}

impl SidebarFrame {
    pub fn new(width: f32) -> Self {
        SidebarFrame {
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .relative()
            .w(px(self.width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .pr(px(theme::GUTTER))
            .children(self.children)
            .child(resize_handle("sidebar-resize", SidebarResize).right_0())
    }
}

/// The drag payload while the sidebar's right border is being dragged.
#[derive(Clone)]
pub struct SidebarResize;

impl Render for SidebarResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
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

#[cfg(test)]
mod tests {
    use gpui_kit::{Corners, Edges, TestAppContext, VisualTestContext, point, px};

    use crate::settings::{self, Tabs};
    use crate::shell::sidebar::tests::painted;
    use crate::test_support::{draw, shell_on_chat};
    use crate::theme;

    /// The sidebar is on the very glass the rail is on, the top rail or the sidebar's own top row,
    /// with nothing of its own between, down to the margin before the islands, wherever the tabs
    /// are.
    #[gpui_kit::test]
    fn the_sidebar_is_one_with_the_rail_and_the_margin_before_the_islands(cx: &mut TestAppContext) {
        let (window, _) = shell_on_chat("sidebar-one-with-the-rail", cx);
        for tabs in Tabs::ALL {
            cx.update(|cx| settings::update(cx, |settings| settings.tabs = tabs));
            draw(window, cx);
            let visual = &mut VisualTestContext::from_window(window, cx);
            let rail = match tabs {
                Tabs::TopRail => "rail-title",
                Tabs::Sidebar => "sidebar-top",
            };
            let rail = visual.debug_bounds(rail).unwrap().center();
            let surface = visual.debug_bounds("sidebar").unwrap();
            let card = visual.debug_bounds("shell-card").unwrap();
            let glass = painted(rail, window, cx);
            assert_eq!(painted(surface.center(), window, cx), glass, "{tabs:?}");
            let mut x = surface.right() - px(1.);
            while x < card.left() {
                let at = point(x, surface.center().y);
                assert_eq!(painted(at, window, cx), glass, "{tabs:?}, no hairline at {x:?}");
                x += px(0.5);
            }
        }
    }

    /// The island beside the sidebar is a rounded card of its own, every corner of it, the left
    /// ones where it meets the sidebar and the rail among them: nothing squares or clips one, and
    /// it has no edge.
    #[gpui_kit::test]
    fn the_island_beside_the_sidebar_is_rounded_at_every_corner(cx: &mut TestAppContext) {
        let (window, _) = shell_on_chat("island-beside-the-sidebar", cx);
        let card = cx.read(|cx| theme::palette(cx).card);
        for tabs in Tabs::ALL {
            cx.update(|cx| settings::update(cx, |settings| settings.tabs = tabs));
            draw(window, cx);
            let visual = &mut VisualTestContext::from_window(window, cx);
            let at = visual.debug_bounds("shell-card").unwrap();
            let sidebar = visual.debug_bounds("sidebar").unwrap();
            assert_eq!(at.left(), sidebar.right() + px(theme::GUTTER), "{tabs:?}");
            let island = visual.update(|window, _| {
                let at = at.scale(window.scale_factor());
                let radius = px(theme::CARD_RADIUS).scale(window.scale_factor());
                let quads = window.painted_quads();
                let island = quads.into_iter().find(|quad| quad.bounds == at);
                island.map(|island| (island, radius))
            });
            let (island, radius) = island.unwrap_or_else(|| panic!("{tabs:?}: no island over {at:?}"));
            assert_eq!(island.background, card.into(), "{tabs:?}");
            assert_eq!(island.corner_radii, Corners::all(radius), "{tabs:?}");
            assert_eq!(island.border_widths, Edges::default(), "{tabs:?}");
            let shown = island.content_mask.bounds.intersect(&island.bounds);
            assert_eq!(shown, island.bounds, "{tabs:?}");
        }
    }
}

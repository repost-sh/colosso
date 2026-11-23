//! The sidebar's frame, the Orca and the Arc layout's: its width, the chrome the layout gives
//! it, its own surface, the grab strip that resizes it, and its slide in and out. The grab strip
//! resizes the changes panel too. Settings' own pages are the Settings tab's content, on the
//! card, not in this frame (`settings_view/page_list.rs`).
//!
//! The surface is the sidebar's in either layout: the part of Orca's card it fills, and in Arc's
//! what's on the glass short of the margin before the cards. It's in the card's colour as opaque
//! as Settings → Appearance → Sidebar glass says, clear by default, and while Sidebar blur is off
//! macOS's own it has a blur of its own behind it as strong as that says, which the frame is
//! given ([`SidebarFrame::blur`]).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Empty, Hsla, InteractiveElement as _,
    IntoElement, ParentElement, Render, RenderOnce, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};

use crate::settings::{self, Layout};
use crate::theme;

/// A sidebar of the given width with the chrome its layout puts around it: inside the card
/// with a hairline in Orca, on the glass in Arc.
#[derive(IntoElement)]
pub struct SidebarFrame {
    layout: Layout,
    width: f32,
    blur: Option<AnyElement>,
    children: Vec<AnyElement>,
}

impl SidebarFrame {
    pub fn new(layout: Layout, width: f32) -> Self {
        SidebarFrame {
            layout,
            width,
            blur: None,
            children: Vec::new(),
        }
    }

    /// Has `blur` fill the sidebar's surface, under its colour: what blurs the desktop behind it.
    pub fn blur(mut self, blur: Option<AnyElement>) -> Self {
        self.blur = blur;
        self
    }
}

/// The colour of the sidebar's own surface: the card's, as opaque as Sidebar glass says.
fn fill(cx: &App) -> Hsla {
    let card = theme::palette(cx).card;
    card.alpha(settings::get(cx).sidebar_opacity(card.a))
}

impl ParentElement for SidebarFrame {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SidebarFrame {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        let margin = match self.layout {
            Layout::Orca => 0.,
            Layout::Arc => theme::GUTTER,
        };
        let surface = div()
            .debug_selector(|| "sidebar-surface".to_owned())
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .right(px(margin))
            .bg(fill(cx))
            .children(self.blur);
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
                Layout::Arc => frame.pr(px(margin)),
            })
            .child(surface)
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

/// A grab strip down the side of a panel, which must be `relative`, dragging `payload` so the
/// drag says which panel it resizes. Place it on the panel's left or right edge.
pub fn resize_handle<T: Render + Clone>(id: &'static str, payload: T) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(|| id.to_owned())
        .absolute()
        .top_0()
        .bottom_0()
        .w(px(6.))
        .cursor_col_resize()
        .on_drag(payload, |payload, _, _, cx| {
            let payload = payload.clone();
            cx.new(|_| payload)
        })
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
    use gpui_kit::{TestAppContext, VisualTestContext, px};

    use crate::settings::{self, GLASS_CLEAR, GLASS_OWN, Layout};
    use crate::test_support::{draw, shell_on};
    use crate::theme;

    /// The sidebar has a surface of its own in either layout, Orca's the part of the card it fills
    /// and Arc's short of the margin before the cards, in the card's colour as opaque as Sidebar
    /// glass says: clear by default, as Arc's sidebar has always been on the glass, so Orca's stands
    /// apart from the content on the card's own; the card's own at the middle, solid at the right.
    #[gpui_kit::test]
    fn the_sidebar_is_as_see_through_as_its_glass_says_in_either_layout(cx: &mut TestAppContext) {
        let (window, _, _) = shell_on("sidebar-glass", Layout::Orca, cx);
        for layout in Layout::ALL {
            let set = |glass, cx: &mut TestAppContext| {
                cx.update(|cx| {
                    settings::update(cx, |settings| {
                        (settings.layout, settings.sidebar_glass) = (layout, glass);
                    });
                });
                draw(window, cx);
                (cx.read(super::fill), cx.read(|cx| theme::palette(cx).card))
            };
            let (fill, card) = set(GLASS_CLEAR, cx);
            let visual = &mut VisualTestContext::from_window(window, cx);
            let surface = visual.debug_bounds("sidebar-surface").unwrap();
            let edge = visual.debug_bounds("sidebar-resize").unwrap();
            let margin = match layout {
                Layout::Orca => px(0.),
                Layout::Arc => px(theme::GUTTER),
            };
            assert_eq!(surface.right(), edge.right() - margin, "{layout:?}");
            assert_eq!(fill, card.alpha(0.), "{layout:?} clear by default");
            assert!(card.a > 0.5, "{layout:?} the content's card is far from clear");
            for (glass, opacity) in [(GLASS_OWN, card.a), (1., 1.)] {
                let (fill, card) = set(glass, cx);
                assert_eq!(fill, card.alpha(fill.a), "{layout:?} at {glass}");
                assert!((fill.a - opacity).abs() < 1e-6, "{layout:?} at {glass}: {fill:?}");
            }
            set(GLASS_CLEAR, cx);
        }
    }
}

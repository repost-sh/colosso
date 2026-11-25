//! The pinned pages, at the sidebar's head (`docs/sidebar.md` 11): a tile each, in a grid
//! they're dragged about in (`shell::TileGrid`), common to every space, so they stay put as a
//! space swipes away. The one shown is lit. A page's tab dragged from a worktree's list, or a
//! loose page's row, onto them is pinned where the tiles make room for it; a tile dragged off
//! them onto a worktree's tabs or the space's loose pages is unpinned there. With none pinned,
//! a dotted zone says where one goes, which the person can put away: it still shows once a
//! page's tab is taken off its list. Hovered, a tile says its page's title and host;
//! right-clicked, it's pinned elsewhere, closed, unpinned, copied into the selected worktree or
//! removed, a menu the top rail's pinned pages have too (`shell/pinned/menu.rs`).

use std::rc::Rc;

use gpui_kit::base::ElementExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Div, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, Point, Render, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};

use super::hover::Hovered;
use super::rows::TAB_ICON;
use super::{Look, ROUNDED, shown_on_hover};
use crate::model::Pinned;
use crate::pane::{ROW_GLYPH, row_sized, sized_close_button};
use crate::shell::{
    Arrange, Face, Landing, Shell, TileHand, drag_chip, hides_pinned_zone, pinned_picture,
    pinned_zone_says,
};
use crate::theme;

/// A tile's height, the room between tiles, and the picture in one, the size of a tab row's
/// icon.
const TILE: f32 = 44.;
const GAP: f32 = 8.;
const PICTURE: f32 = TAB_ICON;

/// How far the outline that says a tab can be let go on the tiles stands out from them.
const OUTLINE: f32 = 4.;

/// How many tiles a row holds for `count` pinned pages: up to four, three for five or six, so
/// the rows come out even.
fn per_row(count: usize) -> usize {
    match count {
        5 | 6 => 3,
        count => count.clamp(1, 4),
    }
}

/// A pinned page's tile in hand. On its grid nothing's drawn for it under the pointer, as the
/// grid draws it there; taken well away, where its tile has gone back to its slot, it travels as
/// a chip, to let go on a worktree's tabs.
#[derive(Clone)]
pub struct DraggedPin {
    /// The pinned page's, as its tile is known.
    pub key: u64,
    face: Face,
    hand: Rc<TileHand>,
    /// Where in the tile the pointer took hold.
    grab: Point<Pixels>,
}

impl Render for DraggedPin {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.hand.away() {
            return div();
        }
        drag_chip(
            std::slice::from_ref(&self.face),
            self.grab,
            theme::palette(cx),
        )
    }
}

/// The pinned pages' region: their tiles, or the zone that says where one goes; none while no
/// page is pinned, the zone was put away and nothing it takes is in hand. `taking` says a page's
/// tab or a loose page's row is in hand, off its list: the region says so, and lights under it,
/// and over it the tiles make room for it where it would land.
pub fn region(
    shell: &Shell,
    taking: bool,
    look: Look,
    cx: &mut Context<Shell>,
) -> Option<Stateful<Div>> {
    let region = shell.pinned_region("sidebar-pinned", (taking, look.tile_hover), cx)?;
    let region = region.relative().rounded(px(10.));
    let grid = ("sidebar-pinned-grid", (Arrange::Grid(per_row), px(GAP)));
    let landing = Landing {
        accent: look.p.accent,
        corners: px(ROUNDED),
    };
    let tile = |at: (usize, &Pinned), shown, cx: &mut Context<Shell>| {
        tile(shell, at, shown, look, cx).into_any_element()
    };
    let Some(grid) = shell.pinned_grid(grid, (taking, landing), tile, cx) else {
        return Some(region.child(zone(taking, look)));
    };
    let outline = div()
        .absolute()
        .top(px(-OUTLINE))
        .bottom(px(-OUTLINE))
        .left(px(-OUTLINE))
        .right(px(-OUTLINE))
        .rounded(px(10. + OUTLINE))
        .border_1()
        .border_dashed()
        .border_color(look.label.opacity(0.6));
    Some(
        region
            .child(grid)
            .when(taking, |region| region.child(outline)),
    )
}

/// The zone that says where a pinned page goes, while none is pinned: a dotted outline a tile
/// high, saying so, with a × that puts it away, shown while the pointer's on it. With a page's
/// tab in hand it says to let it go there, and has no ×.
fn zone(taking: bool, look: Look) -> Div {
    let p = look.p;
    let dismiss = row_sized(sized_close_button("sidebar-pinned-dismiss", ROW_GLYPH - 4., p))
        .debug_selector(|| "sidebar-pinned-dismiss".to_owned());
    let end = div()
        .absolute()
        .top_0()
        .bottom_0()
        .right(px(6.))
        .flex()
        .items_center();
    let end = shown_on_hover(end, "sidebar-pinned-zone").child(hides_pinned_zone(dismiss));
    div()
        .group("sidebar-pinned-zone")
        .debug_selector(|| "sidebar-pinned-zone".to_owned())
        .h(px(TILE))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(10.))
        .border_1()
        .border_dashed()
        .border_color(look.label.opacity(if taking { 0.6 } else { 0.35 }))
        .text_xs()
        .text_color(look.label.opacity(if taking { 1. } else { 0.8 }))
        .child(pinned_zone_says(taking))
        .when(!taking, |zone| zone.child(end))
}

/// Has `row` take a pinned page's tile let go on it, lit under it, `unpinned` doing with the
/// page, by its key, what's done with one let go there: unpinned among a branch's tabs or the
/// loose pages (`docs/sidebar.md` 11).
pub(super) fn takes_pins(
    row: Stateful<Div>,
    look: Look,
    cx: &mut Context<Shell>,
    unpinned: impl Fn(&mut Shell, u64, &mut Context<Shell>) + 'static,
) -> Stateful<Div> {
    row.drag_over::<DraggedPin>(move |style, _, _, _| style.bg(look.row_hover))
        .on_drop(cx.listener(move |this, pin: &DraggedPin, _, cx| unpinned(this, pin.key, cx)))
}

/// The tile of the page `pinned`, in slot `at`: a rounded square that lights under the pointer,
/// lit when it's the tab `shown`, lifted off its grid while it's in hand, and noting where it's
/// drawn for the card of what's hovered, which a press puts away.
fn tile(
    shell: &Shell,
    (at, pinned): (usize, &Pinned),
    shown: bool,
    look: Look,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let key = pinned.key();
    let page = pinned.tab.first();
    let hand = shell.pinned_hand.clone();
    let lifted = hand.holding() == Some(key);
    let held = DraggedPin {
        key,
        face: Face::of(page, cx),
        hand: hand.clone(),
        grab: Point::default(),
    };
    let face = pinned_picture(page, PICTURE, look.p);
    let hovered = Hovered::Pin(key);
    let tile = div()
        .id(("sidebar-pinned-tile", key))
        .min_w_0()
        .h(px(TILE))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(ROUNDED))
        .cursor_pointer()
        .when(shown, |tile| tile.bg(look.tile_selected))
        .when(!shown, |tile| {
            tile.bg(look.tile).hover(|style| style.bg(look.tile_hover))
        })
        .when(lifted, |tile| tile.bg(look.tile_selected).shadow_md())
        .on_prepaint(shell.sidebar_hover.noting(hovered))
        .on_hover(cx.listener(move |this, over: &bool, window, cx| {
            this.hover(hovered, *over, window, cx);
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.press_hovered(cx)),
        )
        .debug_selector(move || format!("sidebar-pinned-{at}"))
        .on_drag(held, move |held, grab, _, cx| {
            hand.take(key, grab);
            let chip = DraggedPin {
                grab,
                ..held.clone()
            };
            cx.new(|_| chip)
        })
        .child(face)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.show_tab_holding(key, cx);
        }));
    Shell::with_pinned_menu(tile, key, cx)
}

#[cfg(test)]
mod tests {
    use super::per_row;

    #[test]
    fn rows_come_out_even() {
        assert_eq!(per_row(1), 1);
        assert_eq!(per_row(2), 2);
        assert_eq!(per_row(4), 4);
        assert_eq!(per_row(5), 3);
        assert_eq!(per_row(6), 3);
        assert_eq!(per_row(7), 4);
        assert_eq!(per_row(12), 4);
    }
}

//! The pinned pages, at the sidebar's head (`docs/arc-sidebar.md` 11): a tile each, as the
//! projects' are, in a grid they're dragged about in with the same physics (`shell::TileGrid`),
//! common to every space, so they stay put as a space swipes away. The one shown is lit. A
//! page's tab dragged from a worktree's list, or a loose page's row, onto them is pinned where
//! the tiles make room for it; a tile dragged off them onto a worktree's tabs or the space's
//! loose pages is unpinned there. With none pinned, a dotted zone says where one goes, which the
//! person can put away: it still shows once a page's tab is taken off its list. Hovered, a tile
//! says its page's title and host; right-clicked, it's pinned elsewhere, closed, unpinned,
//! copied into the selected worktree or removed, a menu the Orca layout's pinned pages have too
//! (`shell/pinned/menu.rs`).

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Point, Render, Stateful, StatefulInteractiveElement as _, Styled as _, Window, div,
    px,
};

use super::Look;
use super::hover::Hovered;
use super::tiles::{GAP, PICTURE, TILE, per_row, tile_frame};
use crate::model::Pinned;
use crate::pane::{ROW_GLYPH, row_sized, sized_close_button};
use crate::shell::{
    Arrange, Face, Landing, Shell, TileHand, drag_chip, hides_pinned_zone, pinned_picture,
    pinned_zone_says,
};
use crate::theme;

/// How far the outline that says a tab can be let go on the tiles stands out from them.
const OUTLINE: f32 = 4.;

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
    let region = shell.pinned_region("arc-pinned", (taking, look.tile_hover), cx)?;
    let region = region.relative().rounded(px(10.));
    let grid = ("arc-pinned-grid", (Arrange::Grid(per_row), px(GAP)));
    let landing = Landing {
        accent: look.p.accent,
        corners: px(super::ROUNDED),
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
    let dismiss = row_sized(sized_close_button("arc-pinned-dismiss", ROW_GLYPH - 4., p))
        .debug_selector(|| "arc-pinned-dismiss".to_owned());
    let end = div()
        .absolute()
        .top_0()
        .bottom_0()
        .right(px(6.))
        .flex()
        .items_center()
        .opacity(0.)
        .group_hover("arc-pinned-zone", |style| style.opacity(1.))
        .child(hides_pinned_zone(dismiss));
    div()
        .group("arc-pinned-zone")
        .debug_selector(|| "arc-pinned-zone".to_owned())
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

/// The tile of the page `pinned`, in slot `at`, lit when it's the tab `shown`.
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
    let tile = tile_frame(
        ("arc-pinned-tile", key),
        Hovered::Pin(key),
        (shown, lifted),
        shell,
        look,
        cx,
    )
    .debug_selector(move || format!("arc-pinned-{at}"))
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

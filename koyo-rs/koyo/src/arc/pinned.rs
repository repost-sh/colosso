//! The pinned pages, at the sidebar's head (`docs/arc-sidebar.md` 11): a tile each, as the
//! projects' are, in a grid they're dragged about in with the same physics (`tile_grid`), common
//! to every space, so they stay put as a space swipes away. The one shown is lit. A page's tab
//! dragged from a worktree's list, or a loose page's row, onto them is pinned; a tile dragged
//! off them onto a worktree's tabs or the space's loose pages is unpinned there. With none
//! pinned, a dotted zone says where one goes, which the person can put away: it still shows
//! once a page's tab is taken off its list. Hovered, a tile says its page's title and host;
//! right-clicked, it's pinned elsewhere, closed, unpinned or removed.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Point, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, img, px,
};

use super::Look;
use super::actions::{ClosePinned, EditPinned, RemovePinned, RepinAtCurrent, Unpin};
use super::hover::Hovered;
use super::rename;
use super::tile_grid::{TileGrid, TileHand};
use super::tiles::{GAP, PICTURE, TILE, per_row, tile_frame};
use crate::browser::{Address, read_address};
use crate::history;
use crate::model::{Pane, Pinned};
use crate::pane::{ROW_GLYPH, row_sized, sized_close_button};
use crate::projects::with_menu;
use crate::settings;
use crate::shell::{Dragged, DraggedPane, FAVICON_CORNERS, Face, Shell, drag_chip, icon};
use crate::theme;

/// How far the outline that says a tab can be let go on the tiles stands out from them.
const OUTLINE: f32 = 4.;
/// How much of its strength the picture of a page that isn't open keeps.
const CLOSED: f32 = 0.6;

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
/// tab or a loose page's row is in hand, off its list: the region says so, and lights under it.
pub fn region(
    shell: &Shell,
    taking: bool,
    look: Look,
    cx: &mut Context<Shell>,
) -> Option<Stateful<Div>> {
    let pinned = shell.workspace.pinned();
    if pinned.is_empty() && !taking && settings::get(cx).pinned_zone_dismissed {
        return None;
    }
    // Where it's drawn is where a tab in hand is off its list, to let go here.
    let hand = shell.tab_hand.clone();
    let note = move |bounds: Bounds<Pixels>, _: &mut Window, _: &mut App| hand.taken_at(bounds);
    let region = div()
        .id("arc-pinned")
        .debug_selector(|| "arc-pinned".to_owned())
        .flex_shrink_0()
        .relative()
        .rounded(px(10.))
        .drag_over::<DraggedPane>(move |style, held, _, _| {
            if taking && matches!(held.what, Dragged::Tab(..) | Dragged::Loose) {
                style.bg(look.tile_hover)
            } else {
                style
            }
        })
        .on_drop(cx.listener(|this, held: &DraggedPane, _, cx| {
            if let (Dragged::Tab(..) | Dragged::Loose, [face]) = (held.what, held.faces.as_slice())
            {
                this.pin_tab_of(face.id, cx);
            }
        }))
        .on_prepaint(note);
    // The visual check points at the region where it's drawn.
    #[cfg(feature = "visual-check")]
    let region = region.on_prepaint(crate::visual::noting("arc-pinned".into()));
    if pinned.is_empty() {
        return Some(region.child(zone(taking, look)));
    }
    let shown = shell.workspace.pinned_shown();
    let tiles = (pinned.iter().enumerate())
        .map(|(at, pinned)| {
            let tile = tile(shell, (at, pinned), shown == Some(at), look, cx);
            (pinned.key(), tile.into_any_element())
        })
        .collect();
    let on_drop = cx.listener(|this, &(from, to): &(usize, usize), _, cx| {
        this.move_pinned(from, to, cx);
    });
    let rows = (per_row(pinned.len()), px(GAP));
    let grid = TileGrid::new(
        "arc-pinned-grid",
        tiles,
        rows,
        shell.pinned_hand.clone(),
        Rc::new(on_drop),
    );
    // Round the tiles, where a tab let go joins them, without moving them.
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
    let says = if taking {
        "Drop to pin"
    } else {
        "Drag a page here to pin it"
    };
    let dismiss = row_sized(sized_close_button("arc-pinned-dismiss", ROW_GLYPH - 4., p))
        .debug_selector(|| "arc-pinned-dismiss".to_owned())
        .tooltip(|window, cx| Tooltip::new("Hide").build(window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| {
            settings::update(cx, |settings| settings.pinned_zone_dismissed = true);
        });
    let end = div()
        .absolute()
        .top_0()
        .bottom_0()
        .right(px(6.))
        .flex()
        .items_center()
        .opacity(0.)
        .group_hover("arc-pinned-zone", |style| style.opacity(1.))
        .child(dismiss);
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
        .child(says)
        .when(!taking, |zone| zone.child(end))
}

/// The first letter of the host of the page at `address`, which a site with no icon shows.
fn initial(address: &str) -> Option<SharedString> {
    let host = history::host(address)?;
    let first = host.chars().find(|first| first.is_alphanumeric())?;
    Some(first.to_uppercase().collect::<String>().into())
}

/// A pinned page's picture, the size of a tab row's icon: its site's icon, its corners a little
/// rounded; with none, its host's initial; a globe for a page with no host, as a new tab's
/// start page.
fn picture(page: &Pane, look: Look, cx: &App) -> AnyElement {
    if let Some(favicon) = page.favicon(cx) {
        return img(favicon)
            .size(px(PICTURE))
            .rounded(px(FAVICON_CORNERS))
            .into_any_element();
    }
    let muted = div().text_color(look.p.muted);
    match initial(&page.address(cx).unwrap_or_default()) {
        Some(initial) => muted
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .child(initial)
            .into_any_element(),
        None => muted
            .child(icon(IconName::Globe, PICTURE))
            .into_any_element(),
    }
}

/// The tile of the page `pinned`, in slot `at`, lit when it's the tab `shown`.
fn tile(
    shell: &Shell,
    (at, pinned): (usize, &Pinned),
    shown: bool,
    look: Look,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
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
    // A page that isn't open, closed or not opened since the window came back, shows fainter.
    let open = page.view().is_some();
    let face = div()
        .when(!open, |face| face.opacity(CLOSED))
        .child(picture(page, look, cx));
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
    .on_click(cx.listener(move |this, _, _, cx| this.pick_pinned(key, cx)));
    // Its menu is worked out as it's opened, not as the tile's drawn.
    let shell = cx.weak_entity();
    with_menu(tile, move |_, cx| {
        let menu = shell.read_with(cx, |shell, cx| Menu::of(shell, key, cx).build());
        menu.unwrap_or_else(|_| NativeMenu::new())
    })
}

/// Has the window's root handle what a pinned page's menu picks.
pub(super) fn handle_actions(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(
        cx.listener(|this, ClosePinned(key): &ClosePinned, window, cx| {
            this.close_apart(*key, Shell::close_pinned, window, cx);
        }),
    )
    .on_action(cx.listener(|this, Unpin(key): &Unpin, _, cx| this.unpin(*key, cx)))
    .on_action(
        cx.listener(|this, RemovePinned(key): &RemovePinned, window, cx| {
            this.close_apart(*key, Shell::remove_pinned, window, cx);
        }),
    )
    .on_action(
        cx.listener(|this, RepinAtCurrent(key): &RepinAtCurrent, _, cx| {
            this.repin_at_current(*key, cx);
        }),
    )
    .on_action(
        cx.listener(|this, EditPinned(key): &EditPinned, window, cx| {
            this.ask_pinned_address(*key, window, cx);
        }),
    )
}

impl Shell {
    /// Asks for the address the pinned page known by `key` is pinned at, the one it has in the
    /// field: an address typed as in the address bar, which it's then pinned at. Left empty, or
    /// words that are no address, it stays pinned where it was.
    fn ask_pinned_address(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.workspace.pinned_at(key) else {
            return;
        };
        let home = self.workspace.pinned()[at].home.clone();
        let shell = cx.weak_entity();
        let done = move |typed: String, cx: &mut App| {
            let Some(Address::Page(address)) = read_address(&typed) else {
                return;
            };
            (shell.update(cx, |shell, cx| shell.repin_at(key, address, cx))).ok();
        };
        let asks = ("Edit Pinned Page", "Save");
        rename::ask(asks, home, "Address", window, cx, done);
    }
}

/// What a pinned page's menu offers (`docs/arc-sidebar.md` 11), and which of it can be picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Menu {
    key: u64,
    /// Whether its page shows another address than the one it's pinned at, to pin it at.
    pub strayed: bool,
    /// Whether it has a page to close.
    pub open: bool,
}

impl Menu {
    pub(super) fn of(shell: &Shell, key: u64, cx: &App) -> Menu {
        Menu {
            key,
            strayed: shell.pinned_strayed(key, cx).is_some(),
            open: shell.pinned_is_open(key, cx),
        }
    }

    /// The menu, as Arc's for a pinned tab: the address it's pinned at, to change; then closing
    /// it, which leaves it pinned, and unpinning it; and removing it, apart.
    fn build(self) -> NativeMenu {
        let key = self.key;
        let edit = NativeMenu::new()
            .menu_with_disabled(
                "Replace Pinned URL with Current",
                !self.strayed,
                Box::new(RepinAtCurrent(key)),
            )
            .menu("Edit…", Box::new(EditPinned(key)));
        NativeMenu::new()
            .submenu("Edit Pinned Page", edit)
            .separator()
            .menu_with_disabled("Close", !self.open, Box::new(ClosePinned(key)))
            .menu("Unpin", Box::new(Unpin(key)))
            .separator()
            .menu("Remove Pinned Tab", Box::new(RemovePinned(key)))
    }
}

#[cfg(test)]
mod tests {
    use super::initial;

    #[test]
    fn a_site_with_no_icon_shows_its_hosts_first_letter() {
        let letter = |address| initial(address).map(|letter| letter.to_string());
        assert_eq!(letter("https://www.example.com/docs").as_deref(), Some("E"));
        assert_eq!(letter("http://localhost:3000/").as_deref(), Some("L"));
        assert_eq!(letter("https://127.0.0.1:8080/").as_deref(), Some("1"));
        assert_eq!(letter("about:blank"), None, "no host: a globe");
        assert_eq!(letter(""), None);
    }
}

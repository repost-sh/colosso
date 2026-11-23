//! The pinned pages in the Orca layout's top rail (`docs/arc-sidebar.md` 1, 11), the pages the
//! Arc sidebar's tiles show, every project's: a button each, its site's icon alone, in a pill
//! lighter than the rail that tells them from a checkout's tabs. The one shown is lit; hovered,
//! a button says its page's title; picked, its page shows; dragged, it moves among them
//! (`shell::TileGrid`); right-clicked, it has the tile's menu. The pill is a rounded rectangle
//! with no edge, its buttons as far from its edge on every side, their corners running along its
//! own. A page's tab or a loose page let go on the pill is pinned where the buttons make room
//! for it. With none pinned, a pill of the same shape says where one goes, which the person can
//! put away: it's back, edged in dashes, while a page's tab is in hand.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Div, Hsla, InteractiveElement as _, IntoElement as _,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _, div, px,
};

use crate::model::Pinned;
use crate::pane::close_button;
use crate::shell::{
    Arrange, InHand, Landing, Shell, hides_pinned_zone, pinned_picture, pinned_zone_says,
};
use crate::theme::{Palette, gray};

/// A button's side, and the room between two.
const BUTTON: f32 = 26.;
const GAP: f32 = 2.;
/// The pill's corners: a rounded rectangle rather than a capsule, so what's in it, as far from
/// its edge on every side, has corners that run along its own.
const PILL: f32 = 9.;
/// How far a button is from the pill's edge, on every side.
const INSET: f32 = 3.;
/// The corners of a button, of the slot lit where a page let go would land and of the zone's ×,
/// each `INSET` in from the pill's edge: the tab rows' 6 pt.
const CORNERS: f32 = PILL - INSET;
/// A page's icon on its button, as big as a tile's in the Arc sidebar.
const PICTURE: f32 = 16.;

/// The shades the pill lays on the rail, lighter than it in either appearance.
#[derive(Clone, Copy)]
struct Shades {
    p: Palette,
    pill: Hsla,
    /// The button of the page shown.
    lit: Hsla,
    /// A button under the pointer, or in hand.
    hover: Hsla,
    /// The pill under a page's tab that it takes.
    taking: Hsla,
}

impl Shades {
    fn of(p: Palette, cx: &App) -> Shades {
        if cx.theme().mode.is_dark() {
            Shades {
                p,
                pill: gray(1., 0.07),
                lit: gray(1., 0.18),
                hover: gray(1., 0.1),
                taking: gray(1., 0.12),
            }
        } else {
            Shades {
                p,
                pill: gray(1., 0.55),
                lit: gray(1., 0.95),
                hover: gray(1., 0.75),
                taking: gray(1., 0.75),
            }
        }
    }
}

/// The pinned pages' pill, or the zone that says where one goes; none while no page is pinned,
/// the zone was put away and nothing it takes is in hand. `taking` says a page's tab or a loose
/// page is in hand, off its list: the pill lights under it, and its buttons make room for it
/// where it would land.
pub(super) fn region(
    shell: &Shell,
    taking: bool,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Option<Stateful<Div>> {
    let shades = Shades::of(p, cx);
    let region = (shell.pinned_region("orca-pinned", (taking, shades.taking), cx)?)
        .h(px(BUTTON + 2. * INSET))
        .flex()
        .items_center()
        .rounded(px(PILL));
    let row = ("orca-pinned-grid", (Arrange::Row, px(GAP)));
    let landing = Landing {
        accent: p.accent,
        corners: px(CORNERS),
    };
    let button = |at: (usize, &Pinned), shown, cx: &mut Context<Shell>| {
        button(shell, at, shown, shades, cx).into_any_element()
    };
    let Some(grid) = shell.pinned_grid(row, (taking, landing), button, cx) else {
        return Some(region.child(zone(taking, shades)));
    };
    Some(region.px(px(INSET)).bg(shades.pill).child(grid))
}

/// The zone that says where a pinned page goes, while none is: the pill's shape and fill, saying
/// so, with a × that puts it away, shown while the pointer's on it, as far in from its edge as a
/// button is in the pill. With a page's tab in hand it says to let it go there, edged in dashes,
/// and has no ×.
fn zone(taking: bool, shades: Shades) -> Div {
    let p = shades.p;
    let dismiss = close_button("orca-pinned-dismiss", p)
        .size(px(BUTTON))
        .rounded(px(CORNERS))
        .debug_selector(|| "orca-pinned-dismiss".to_owned())
        .opacity(0.)
        .group_hover("orca-pinned-zone", |style| style.opacity(1.));
    div()
        .group("orca-pinned-zone")
        .debug_selector(|| "orca-pinned-zone".to_owned())
        .h_full()
        .pl_3()
        .pr(px(if taking { 12. } else { INSET }))
        .flex()
        .items_center()
        .gap_1()
        .rounded(px(PILL))
        .bg(shades.pill)
        .when(taking, |zone| {
            zone.border_1()
                .border_dashed()
                .border_color(p.muted.opacity(0.9))
        })
        .text_xs()
        .text_color(if taking { p.text } else { p.muted })
        .child(pinned_zone_says(taking))
        .when(!taking, |zone| zone.child(hides_pinned_zone(dismiss)))
}

/// The button of the page `pinned`, in slot `at`, lit when it's the tab `shown`.
fn button(
    shell: &Shell,
    (at, pinned): (usize, &Pinned),
    shown: bool,
    shades: Shades,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let key = pinned.key();
    let page = pinned.tab.first();
    let title = page.title.clone();
    let hand = shell.pinned_hand.clone();
    let lifted = hand.holding() == Some(key);
    let button = div()
        .id(("orca-pinned-button", key))
        .debug_selector(move || format!("orca-pinned-{at}"))
        .size(px(BUTTON))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(CORNERS))
        .cursor_pointer()
        .when(shown, |button| button.bg(shades.lit))
        .when(!shown, |button| button.hover(|style| style.bg(shades.hover)))
        .when(lifted, |button| button.bg(shades.hover).shadow_sm())
        .child(pinned_picture(page, PICTURE, shades.p))
        .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.show_tab_holding(key, cx);
        }))
        .on_drag(InHand, move |_, grab, _, cx| {
            hand.take(key, grab);
            cx.new(|_| InHand)
        });
    #[cfg(feature = "visual-check")]
    let button = {
        use gpui_kit::base::ElementExt as _;
        button.on_prepaint(crate::visual::noting(format!("orca-pinned-{at}")))
    };
    Shell::with_pinned_menu(button, key, cx)
}

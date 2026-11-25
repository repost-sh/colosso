//! The tabs in the sidebar, while **Tabs** puts them there (`docs/layout.md`, Tabs): the pinned
//! pages as tiles over the rows that scroll, the unpinned pages as rows at their head, the Settings
//! tab's row while it's open, then New Tab's, and under each branch's row its tabs' rows, what's in
//! each a step further in than in the branch's, sliding closed as a click on the branch's row
//! collapses them and open as the next brings them back. With the tabs on the top rail none of them
//! are here, the rail having them (`top_rail::pages`, `top_rail::tabs`), and a click on a branch's
//! row selects the branch. The rows are `pinned.rs`'s, `unpinned.rs`'s and `rows.rs`'s
//! (`rows::branch_tabs`). Right-clicked where nothing has a menu of its own, the tab area opens
//! its menu (`layout_menus::tab_area`).

use gpui_kit::{
    App, Context, Div, InteractiveElement as _, ParentElement as _, Stateful, Styled, div, px,
};

use super::{Look, rows};
use crate::settings::{self, LoosePages};
use crate::shell::{Shell, tab_area};

/// Between the sidebar's parts: the address bar, the pinned pages, the unpinned pages and the
/// projects.
pub(super) const GAP: f32 = 10.;

/// The pinned pages' tiles over the rows that scroll: they stay put as the rows scroll and as the
/// space changes (`pinned::region`).
pub(super) fn pinned(shell: &Shell, look: Look, cx: &mut Context<Shell>) -> Option<Stateful<Div>> {
    let tiles = super::pinned::region(shell, shell.pinned_taking(), look, cx)?;
    Some(tab_area(tiles.mb(px(GAP))))
}

/// The unpinned pages' rows, the Settings tab's while it's open, and New Tab's, at the head of the
/// rows that scroll, the one at `shown` among them a pill: with no page, the spot that takes the
/// first while a page's tab is in hand (`unpinned::list`). Each space's own, they slide with it as
/// it changes; shared, they stay put, as the pinned pages do (`Shell::carry_loose`).
pub(super) fn unpinned(
    shell: &Shell,
    shown: Option<usize>,
    look: Look,
    cx: &mut Context<Shell>,
) -> Div {
    let pages = super::unpinned::list(shell, (shown, shell.loose_taking()), look, cx);
    let unpinned = div()
        .debug_selector(|| "sidebar-unpinned".to_owned())
        .flex_shrink_0()
        .pb(px(GAP))
        .flex()
        .flex_col()
        .gap(px(rows::GAP))
        .children(pages)
        .child(super::unpinned::new_tab(look));
    shell.carry_loose(tab_area(unpinned), cx)
}

impl Shell {
    /// `piece`, the loose pages or what holds them, on the top rail or in the sidebar: moving
    /// with the sidebar's space on a switch while each space lists its own, and staying put, as
    /// the pinned pages do, while every space shares them (`docs/layout.md`).
    pub(in crate::shell) fn carry_loose<E: Styled>(&self, piece: E, cx: &App) -> E {
        match settings::get(cx).loose_pages {
            LoosePages::PerSpace => self.carry(piece, cx),
            LoosePages::Shared => piece,
        }
    }
}

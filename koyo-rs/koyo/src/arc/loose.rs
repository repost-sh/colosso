//! The loose pages of the space shown (`docs/arc-sidebar.md` 11): the pages open in it that are
//! no project's, as rows under the pinned pages and over the open project, as Arc lists a
//! space's tabs. The one shown is a pill, as a worktree's tab is. A row is dragged among them, onto
//! a worktree's tabs, where it's a tab of that worktree, or onto the pinned pages, where it's
//! pinned; and a page's tab from a worktree, or a pinned page's tile, let go on them is one of
//! them. Right-clicked, a row is copied into the selected worktree. With none, there's no list:
//! a dashed strip, "Drop to keep it here", takes one that's in hand. Each space's own, they're
//! part of the space, and slide with it as it changes; shared, they stay where they are with the
//! pinned pages. The Settings tab, while it's open, is the row after them, the pill, as it's
//! shown (11.9).

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Axis, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Stateful, StatefulInteractiveElement as _, Styled as _, div, px, relative,
};

use super::worktrees::{ROW_END, TAB_ICON, arc_row, pill_row, tab_icon};
use super::{DraggedPin, Look};
use crate::model::{Front, Loose, SettingsTab};
use crate::pane::{ROW_GLYPH, row_sized, sized_close_button};
use crate::projects::with_menu;
use crate::shell::{
    CopyToWorktree, Shell, ellipsized, icon, loose_close, loose_list, loose_row, loose_spot,
    settings_close, settings_row, takes_tabs, title_with_end,
};
/// Between the rows, as between a worktree's.
const GAP: f32 = 2.;
/// Under the last of them, before the open project's name.
const AFTER: f32 = 8.;
/// The most of the sidebar's height the shared ones take before they scroll, so the space's
/// project and tiles keep the rest.
const MOST: f32 = 0.4;

/// The loose pages every space shares, which stay where they are as a space slides, as the
/// pinned pages over them do: between those and the space, scrolling on their own once they'd
/// take more than `MOST` of the sidebar. None while each space lists its own.
pub fn still(
    shell: &Shell,
    taking: bool,
    look: Look,
    cx: &mut Context<Shell>,
) -> Option<Stateful<Div>> {
    if !shell.workspace.loose_shared() {
        return None;
    }
    let list = list(shell, taking, look, cx)?;
    let still = div()
        .id("arc-loose-still")
        .flex_shrink_0()
        .max_h(relative(MOST))
        .overflow_y_scroll()
        .child(list);
    Some(still)
}

/// The space's own loose pages, which slide with it, over its project. None while every space
/// shares them.
pub fn sliding(shell: &Shell, taking: bool, look: Look, cx: &mut Context<Shell>) -> Option<Div> {
    if shell.workspace.loose_shared() {
        return None;
    }
    let list = list(shell, taking, look, cx)?;
    Some(div().flex_shrink_0().pb(px(AFTER)).child(list))
}

/// The loose pages the space shown lists, then the Settings tab's row while it's open.
fn list(shell: &Shell, taking: bool, look: Look, cx: &mut Context<Shell>) -> Option<Div> {
    let pages = pages(shell, taking, look, cx);
    let settings = shell.workspace.settings().map(|_| settings(look, cx));
    if pages.is_none() && settings.is_none() {
        return None;
    }
    Some(div().flex().flex_col().gap(px(GAP)).children(pages).children(settings))
}

/// The loose pages the space shown lists, in a list they're dragged along; with none, the strip
/// that takes one while something they'd take is in hand, and nothing otherwise. `taking` says
/// something is: a page's tab off a worktree's list, or a pinned page's tile away from the
/// others.
fn pages(shell: &Shell, taking: bool, look: Look, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let pages = shell.workspace.loose();
    if pages.is_empty() {
        return taking.then(|| strip(look, cx).into_any_element());
    }
    let front = shell.workspace.front();
    let rows = (pages.iter().enumerate())
        .map(|(at, page)| {
            let shown = matches!(front, Some(Front::Loose(shown)) if shown == at);
            row(shell, (at, page), (shown, taking), look, cx)
        })
        .collect();
    let list = loose_list(&shell.tab_hand, (Axis::Vertical, px(GAP)), rows, cx);
    Some(list.into_any_element())
}

/// The Settings tab's row: the pill, as it's the tab shown, its gear where a page's icon goes,
/// and its × at its end.
fn settings(look: Look, cx: &mut Context<Shell>) -> Stateful<Div> {
    let p = look.p;
    let close = row_sized(sized_close_button("arc-settings-tab-close", ROW_GLYPH, p));
    let close =
        settings_close(close, cx).debug_selector(|| "arc-settings-tab-close".to_owned());
    let title = (div().id("arc-settings-tab-title").flex()).child(ellipsized(SettingsTab::TITLE));
    let row = arc_row("arc-settings-tab")
        .debug_selector(|| "arc-settings-tab".to_owned())
        .bg(look.pill)
        .pr(px(ROW_END))
        .child(tab_icon(icon(SettingsTab::ICON, TAB_ICON).into_any_element(), p))
        .child(title_with_end(title, close, true, "arc-settings-tab"));
    settings_row(row, cx)
}

/// Has `row` take what's let go on it for the loose pages, at `place` among them: a page's tab
/// from a worktree's list (`takes_tabs`), or a pinned page's tile, which is unpinned there. Lit
/// under either while `taking`.
fn takes_pages(
    row: Stateful<Div>,
    (place, taking): (usize, bool),
    look: Look,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    takes_pins(takes_tabs(row, (place, taking), look.row_hover, cx), place, look, cx)
}

/// Has `row` take a pinned page's tile let go on it, unpinned at `place` among the loose pages,
/// lit under it.
fn takes_pins(
    row: Stateful<Div>,
    place: usize,
    look: Look,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    row.drag_over::<DraggedPin>(move |style, _, _, _| style.bg(look.row_hover))
        .on_drop(cx.listener(move |this, pin: &DraggedPin, _, cx| {
            this.loosen_pinned(pin.key, place, cx);
        }))
}

/// Where a page let go is the space's first loose one, while it has none: the dashed spot that
/// says so, a row of their list, which takes a pinned page's tile too.
fn strip(look: Look, cx: &mut Context<Shell>) -> Stateful<Div> {
    let strip = arc_row("arc-loose-strip").debug_selector(|| "arc-loose-strip".to_owned());
    let strip = loose_spot(strip, (look.label, look.row_hover), cx);
    #[cfg(feature = "visual-check")]
    let strip = {
        use gpui_kit::base::ElementExt as _;
        strip.on_prepaint(crate::visual::noting("arc-loose-strip".into()))
    };
    takes_pins(strip, 0, look, cx)
}

/// The row of the loose page `page`, in place `at`: a pill when it's the tab shown (`active`),
/// tinted while hovered, its × at its end on hover, and always on the one shown.
fn row(
    shell: &Shell,
    (at, page): (usize, &Loose),
    (active, taking): (bool, bool),
    look: Look,
    cx: &mut Context<Shell>,
) -> (u64, AnyElement) {
    let p = look.p;
    let key = page.key();
    let close = row_sized(sized_close_button(("arc-loose-close", key), ROW_GLYPH, p));
    let close = loose_close(close, key, cx)
        .debug_selector(move || format!("arc-loose-close-{at}"))
        .when(!active, |close| {
            close
                .opacity(0.)
                .group_hover("arc-loose", |style| style.opacity(1.))
        });
    let row = arc_row(("arc-loose", key))
        .group("arc-loose")
        .debug_selector(move || format!("arc-loose-{at}"));
    let row = pill_row(row, (key, active), shell, look, cx).pr(px(ROW_END));
    let list = (Axis::Vertical, &shell.tab_hand);
    let row = loose_row(row, (at, page), list, p, cx, |row, face| {
        let title = div()
            .id(("arc-loose-title", key))
            .flex()
            .child(face.label());
        row.child(tab_icon(face.icon(TAB_ICON), p))
            .child(title_with_end(title, close, active, "arc-loose"))
    });
    #[cfg(feature = "visual-check")]
    let row = {
        use gpui_kit::base::ElementExt as _;
        row.on_prepaint(crate::visual::noting(format!("arc-loose-{at}")))
    };
    let shell = cx.weak_entity();
    let row = with_menu(row, move |_, cx| {
        let worktree = shell.read_with(cx, |shell, _| shell.checkout_shown().is_some());
        menu(key, worktree.unwrap_or(false))
    });
    (
        key,
        takes_pages(row, (at, taking), look, cx).into_any_element(),
    )
}

/// What a loose page's menu offers: a copy of it in the selected worktree, while one is.
fn menu(key: u64, worktree: bool) -> NativeMenu {
    NativeMenu::new().menu_with_disabled(
        "Copy to Worktree",
        !worktree,
        Box::new(CopyToWorktree(key)),
    )
}

//! The loose pages of the space shown, while the tabs are in the sidebar (`docs/layout.md`,
//! Sidebar): the pages open in it that are no project's, as rows under the pinned pages and over
//! the projects. The one shown is a pill, as a branch's tab is. A row is dragged among them, onto
//! a worktree's tabs, where it's a tab of that worktree, or onto the pinned pages, where it's
//! pinned; and a page's tab from a worktree, or a pinned page's tile, let go on them is one of
//! them. Right-clicked, a row is copied into the selected worktree. With none, there's no list:
//! a dashed strip, "Drop to keep it here", takes one that's in hand. Each space's own, they're
//! part of the space, and slide with it as it changes; shared, they stay where they are with the
//! pinned pages. The Settings tab, while it's open, is the row after them, the pill while it's
//! shown (11.9); and the last row, New Tab, opens the launcher for a page among them.

use gpui_kit::assets::IconName;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Axis, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Stateful, StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::pinned::takes_pins;
use super::rows::{GAP, ROW_END, TAB_ICON, sidebar_row, tab_icon};
use super::{Look, noted, shown_on_hover};
use crate::launcher::NewUnpinnedPage;
use crate::model::{Loose, SettingsTab};
use crate::pane::{ROW_GLYPH, row_sized, sized_close_button};
use crate::shell::{
    CopyToWorktree, NEW_PAGE, Shell, ellipsized, icon, loose_close, loose_list, loose_row, loose_spot,
    settings_close, settings_row, takes_tabs, title_with_end, with_menu,
};

/// The loose pages the space shown lists, the one at `shown` a pill, then the Settings tab's row
/// while it's open.
pub(crate) fn list(
    shell: &Shell,
    (shown, taking): (Option<usize>, bool),
    look: Look,
    cx: &mut Context<Shell>,
) -> Option<Div> {
    let pages = pages(shell, (shown, taking), look, cx);
    let settings = shell.workspace.settings().map(|_| {
        settings(shell.workspace.settings_shown().is_some(), look, cx)
    });
    if pages.is_none() && settings.is_none() {
        return None;
    }
    Some(div().flex().flex_col().gap(px(GAP)).children(pages).children(settings))
}

/// The row after them, the + and "New Tab", lit under the pointer as theirs are: clicked, it opens
/// the launcher for a page among them, whatever tab is shown (`docs/layout.md`).
pub(super) fn new_tab(look: Look) -> Stateful<Div> {
    let p = look.p;
    look.lit(sidebar_row("sidebar-new-page"), false)
        .debug_selector(|| "sidebar-new-page".to_owned())
        .text_color(p.muted)
        .child(tab_icon(icon(IconName::Plus, TAB_ICON).into_any_element(), p))
        .child(ellipsized("New Tab"))
        .tooltip(|window, cx| {
            Tooltip::new(NEW_PAGE).action(&NewUnpinnedPage, None).build(window, cx)
        })
        .on_click(|_, window, cx| window.dispatch_action(Box::new(NewUnpinnedPage), cx))
}

/// The loose pages the space shown lists, the one at `shown` a pill, in a list they're dragged
/// along; with none, the strip that takes one while something they'd take is in hand, and nothing
/// otherwise. `taking` says something is: a page's tab off a worktree's list, or a pinned page's
/// tile away from the others.
fn pages(
    shell: &Shell,
    (shown, taking): (Option<usize>, bool),
    look: Look,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let pages = shell.workspace.loose();
    if pages.is_empty() {
        return taking.then(|| strip(look, cx).into_any_element());
    }
    let rows = (pages.iter().enumerate())
        .map(|(at, page)| row(shell, (at, page), (shown == Some(at), taking), look, cx))
        .collect();
    let list = loose_list(&shell.tab_hand, (Axis::Vertical, px(GAP)), rows, cx);
    Some(list.into_any_element())
}

/// The Settings tab's row: the pill when it's the tab shown (`active`), its gear where a page's
/// icon goes, and its × at its end on hover, and always while it's shown.
fn settings(active: bool, look: Look, cx: &mut Context<Shell>) -> Stateful<Div> {
    let p = look.p;
    let close = row_sized(sized_close_button("sidebar-settings-tab-close", ROW_GLYPH, p));
    let close = settings_close(close, cx)
        .debug_selector(|| "sidebar-settings-tab-close".to_owned())
        .when(!active, |close| shown_on_hover(close, "sidebar-settings-tab"));
    let title = (div().id("sidebar-settings-tab-title").flex())
        .child(ellipsized(SettingsTab::TITLE));
    let row = sidebar_row("sidebar-settings-tab")
        .group("sidebar-settings-tab")
        .debug_selector(|| "sidebar-settings-tab".to_owned());
    let row = look
        .lit(row, active)
        .pr(px(ROW_END))
        .child(tab_icon(icon(SettingsTab::ICON, TAB_ICON).into_any_element(), p))
        .child(title_with_end(title, close, active, "sidebar-settings-tab"));
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
    let row = takes_tabs(row, (place, taking), look.row_hover, cx);
    takes_pins(row, look, cx, move |this, key, cx| this.loosen_pinned(key, place, cx))
}

/// Where a page let go is the space's first loose one, while it has none: the dashed spot that
/// says so, a row of their list, which takes a pinned page's tile too.
fn strip(look: Look, cx: &mut Context<Shell>) -> Stateful<Div> {
    let strip = (sidebar_row("sidebar-unpinned-strip"))
        .debug_selector(|| "sidebar-unpinned-strip".to_owned());
    let strip = loose_spot(strip, (look.label, look.row_hover), cx);
    let strip = noted(strip, || "sidebar-unpinned-strip".to_owned());
    takes_pins(strip, look, cx, |this, key, cx| this.loosen_pinned(key, 0, cx))
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
    let close = row_sized(sized_close_button(("sidebar-unpinned-close", key), ROW_GLYPH, p));
    let close = loose_close(close, key, cx)
        .debug_selector(move || format!("sidebar-unpinned-close-{at}"))
        .when(!active, |close| shown_on_hover(close, "sidebar-unpinned-row"));
    let row = sidebar_row(("sidebar-unpinned-row", key))
        .group("sidebar-unpinned-row")
        .debug_selector(move || format!("sidebar-unpinned-{at}"));
    let row = look.lit(row, active).pr(px(ROW_END));
    let list = (Axis::Vertical, &shell.tab_hand);
    let row = loose_row(row, (at, page), list, p, cx, |row, face| {
        let title = div()
            .id(("sidebar-unpinned-title", key))
            .flex()
            .child(face.label());
        row.child(tab_icon(face.icon(TAB_ICON), p))
            .child(title_with_end(title, close, active, "sidebar-unpinned-row"))
    });
    let row = noted(row, move || format!("sidebar-unpinned-{at}"));
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

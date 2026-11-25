//! The space's loose pages on the top rail (`docs/sidebar.md` 1, 11), the pages open in it that are
//! no project's, as the sidebar lists them under the pinned pages: tabs as the checkout's are,
//! after the pinned pages' pill and before their + and the checkout's tabs. The one shown is lit;
//! picked, its page shows; dragged, it moves among them, or onto the pill, where it's pinned;
//! closed, by its × or a middle click, it's gone. A page's tab off the checkout's list let go on
//! one lands there; with none, a dashed spot shows where they'd be while one is in hand, and takes
//! it (`docs/layout.md`). The Settings tab, while it's open, comes after them, lit while it's shown
//! (11.9).

use gpui_kit::base::ElementExt as _;
use gpui_kit::{
    AnyElement, Axis, Context, Div, InteractiveElement as _, IntoElement as _, Stateful,
    Styled as _, px,
};

use super::{rail_shape, rail_tab, single_page};
use crate::model::{Front, Loose, SettingsTab};
use crate::pane::close_button;
use crate::shell::{
    List, Shell, ellipsized, icon, loose_close, loose_list, loose_row, loose_spot,
    settings_close, settings_row, takes_tabs,
};
use crate::theme::Palette;

/// The loose pages the space shown lists, as tabs in a list they're dragged along, each taking
/// a page's tab let go on it; with none, the spot that takes the first while a page's tab is in
/// hand, off the checkout's list, and nothing otherwise. A tab of the checkout's held over them
/// is off its list.
pub(super) fn tabs(shell: &Shell, p: Palette, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let taking = shell.loose_taking_tab();
    let pages = shell.workspace.loose();
    if pages.is_empty() {
        return taking.then(|| spot(shell, p, cx).into_any_element());
    }
    let front = shell.workspace.front().and_then(Front::loose);
    let tabs = (pages.into_iter().enumerate())
        .map(|(at, page)| tab(shell, (at, page), (front == Some(at), taking), p, cx))
        .collect();
    let list = (Axis::Horizontal, px(4.));
    Some(loose_list(&shell.tab_hand, list, tabs, cx).into_any_element())
}

/// The tab of the loose page `page`, in place `at`, lit while it's the tab shown (`active`), and
/// under a page's tab in hand while `taking`.
fn tab(
    shell: &Shell,
    (at, page): (usize, &Loose),
    (active, taking): (bool, bool),
    p: Palette,
    cx: &mut Context<Shell>,
) -> (u64, AnyElement) {
    let key = page.key();
    let close = loose_close(close_button(("rail-unpinned-close", key), p), key, cx)
        .debug_selector(move || format!("rail-unpinned-close-{at}"));
    let tab = rail_tab(("rail-unpinned", key), active, p)
        .debug_selector(move || format!("rail-unpinned-{at}"));
    let list = (Axis::Horizontal, &shell.tab_hand);
    let tab = loose_row(tab, (at, page), list, p, cx, |tab, face| {
        single_page(tab, (face.icon(13.), face.label()), None, close)
    });
    #[cfg(feature = "visual-check")]
    let tab = tab.on_prepaint(crate::visual::noting(format!("rail-unpinned-{at}")));
    (key, takes_tabs(tab, (at, taking), p.hover, cx).into_any_element())
}

/// Where the space's first loose page goes while it has none and a page's tab is in hand: a
/// tab high, edged in dashes and saying so. It notes where it's drawn for the tab in hand, which,
/// held over it, is off the checkout's list, as over the loose pages' own.
fn spot(shell: &Shell, p: Palette, cx: &mut Context<Shell>) -> Stateful<Div> {
    let hand = shell.tab_hand.clone();
    let spot = rail_shape("rail-unpinned-spot")
        .debug_selector(|| "rail-unpinned-spot".to_owned())
        .px_3()
        .flex_shrink_0()
        .on_prepaint(move |bounds, _, _| hand.takes_for(List::Loose, bounds));
    loose_spot(spot, (p.text, p.hover), cx)
}

/// The Settings tab, after the loose pages, while it's open: lit while it's the tab shown, its
/// gear where a page's icon goes.
pub(super) fn settings_tab(
    shell: &Shell,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Option<Stateful<Div>> {
    shell.workspace.settings()?;
    let shown = shell.workspace.settings_shown().is_some();
    let close = settings_close(close_button("rail-settings-tab-close", p), cx)
        .debug_selector(|| "rail-settings-tab-close".to_owned());
    let tab = rail_tab("rail-settings-tab", shown, p)
        .debug_selector(|| "rail-settings-tab".to_owned());
    let face = (icon(SettingsTab::ICON, 13.).into_any_element(), ellipsized(SettingsTab::TITLE));
    Some(settings_row(single_page(tab, face, None, close), cx))
}

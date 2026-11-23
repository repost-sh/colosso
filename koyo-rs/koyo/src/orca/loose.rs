//! The space's loose pages in the Orca layout's top rail (`docs/arc-sidebar.md` 1, 11), the
//! pages open in it that are no project's, as the Arc sidebar lists them under the pinned pages:
//! tabs as the checkout's are, after the pinned pages' pill and before a hairline and the
//! checkout's tabs. The one shown is lit; picked, its page shows; dragged, it moves among them,
//! or onto the pill, where it's pinned; closed, by its × or a middle click, it's gone. The
//! Settings tab, while it's open, comes after them, lit, as it's shown (11.9).

use gpui_kit::{
    AnyElement, Axis, Context, InteractiveElement as _, IntoElement as _, Stateful, Div, px,
};

use super::{rail_tab, single_page};
use crate::model::{Front, Loose, SettingsTab};
use crate::pane::close_button;
use crate::shell::{
    Reorder, Shell, ellipsized, icon, loose_close, loose_list, loose_row, settings_close,
    settings_row,
};
use crate::theme::Palette;

/// The loose pages the space shown lists, as tabs in a list they're dragged along; none while
/// it lists none.
pub(super) fn tabs(shell: &Shell, p: Palette, cx: &mut Context<Shell>) -> Option<Reorder> {
    let pages = shell.workspace.loose();
    if pages.is_empty() {
        return None;
    }
    let front = shell.workspace.front();
    let tabs = (pages.into_iter().enumerate())
        .map(|(at, page)| {
            let shown = matches!(front, Some(Front::Loose(shown)) if shown == at);
            tab(shell, (at, page), shown, p, cx)
        })
        .collect();
    let list = (Axis::Horizontal, px(4.));
    Some(loose_list(&shell.tab_hand, list, tabs, cx))
}

/// The tab of the loose page `page`, in place `at`, lit while it's the tab shown (`active`).
fn tab(
    shell: &Shell,
    (at, page): (usize, &Loose),
    active: bool,
    p: Palette,
    cx: &mut Context<Shell>,
) -> (u64, AnyElement) {
    let key = page.key();
    let close = loose_close(close_button(("orca-loose-close", key), p), key, cx)
        .debug_selector(move || format!("orca-loose-close-{at}"));
    let tab = rail_tab(("orca-loose", key), active, p)
        .debug_selector(move || format!("orca-loose-{at}"));
    let list = (Axis::Horizontal, &shell.tab_hand);
    let tab = loose_row(tab, (at, page), list, p, cx, |tab, face| {
        single_page(tab, (face.icon(13.), face.label()), None, close)
    });
    #[cfg(feature = "visual-check")]
    let tab = {
        use gpui_kit::base::ElementExt as _;
        tab.on_prepaint(crate::visual::noting(format!("orca-loose-{at}")))
    };
    (key, tab.into_any_element())
}

/// The Settings tab, after the loose pages, while it's open: lit, as it's the tab shown, its gear
/// where a page's icon goes.
pub(super) fn settings_tab(
    shell: &Shell,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Option<Stateful<Div>> {
    shell.workspace.settings()?;
    let close = settings_close(close_button("orca-settings-tab-close", p), cx)
        .debug_selector(|| "orca-settings-tab-close".to_owned());
    let tab = rail_tab("orca-settings-tab", true, p)
        .debug_selector(|| "orca-settings-tab".to_owned());
    let face = (icon(SettingsTab::ICON, 13.).into_any_element(), ellipsized(SettingsTab::TITLE));
    Some(settings_row(single_page(tab, face, None, close), cx))
}

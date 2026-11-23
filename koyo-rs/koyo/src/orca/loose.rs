//! The space's loose pages in the Orca layout's top rail (`docs/arc-sidebar.md` 1, 11), the
//! pages open in it that are no project's, as the Arc sidebar lists them under the pinned pages:
//! tabs as the checkout's are, after the pinned pages' pill and before a hairline and the
//! checkout's tabs. The one shown is lit; picked, its page shows; dragged, it moves among them,
//! or onto the pill, where it's pinned; closed, by its × or a middle click, it's gone.

use gpui_kit::{AnyElement, Axis, Context, InteractiveElement as _, IntoElement as _, px};

use super::{rail_tab, single_page};
use crate::model::Loose;
use crate::pane::close_button;
use crate::shell::{Reorder, Shell, loose_close, loose_list, loose_row};
use crate::theme::Palette;

/// The loose pages the space shown lists, as tabs in a list they're dragged along; none while
/// it lists none.
pub(super) fn tabs(shell: &Shell, p: Palette, cx: &mut Context<Shell>) -> Option<Reorder> {
    let pages = shell.workspace.loose();
    if pages.is_empty() {
        return None;
    }
    let shown = shell.workspace.loose_shown();
    let tabs = (pages.into_iter().enumerate())
        .map(|(at, page)| tab(shell, (at, page), shown == Some(at), p, cx))
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
        single_page(tab, face, None, close)
    });
    #[cfg(feature = "visual-check")]
    let tab = {
        use gpui_kit::base::ElementExt as _;
        tab.on_prepaint(crate::visual::noting(format!("orca-loose-{at}")))
    };
    (key, tab.into_any_element())
}

//! ⌥↑ and ⌥↓ (`PreviousTab`, `NextTab`) pass over a folded project's tabs in the sidebar
//! (`docs/layout.md`, Keyboard shortcuts). A project's row folding its branches away and saying
//! how it's doing then is `tests::rows`'.

use gpui_kit::TestAppContext;

use super::{act, sidebar_shell, unseen_chat};
use crate::shell::spaces::PreviousTab;
use crate::test_support::{click, draw};

#[gpui_kit::test]
fn option_up_passes_over_a_folded_projects_tabs(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-project-fold-keys"], cx);
    unseen_chat(&shell, cx);
    draw(window, cx);
    let shown = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.shown_tab());
    assert_eq!(shown(cx), Some((0, 0, 1)));

    click(window, "sidebar-project-0", cx);
    act(window, PreviousTab, cx);
    assert_eq!(shown(cx), Some((0, 0, 1)), "folded: no row of its to go to");

    click(window, "sidebar-project-0", cx);
    act(window, PreviousTab, cx);
    assert_eq!(shown(cx), Some((0, 0, 0)), "open: the row above");
}

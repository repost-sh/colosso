//! The project's name in the Arc sidebar (`docs/arc-sidebar.md` 4) folds its worktrees away and
//! brings them back. Folded, it says how the project's doing at its end, as its tile does, where
//! its tabs' rows said it; open, it leaves that to them. ⌥↑ and ⌥↓ (`PreviousTab`, `NextTab`) pass
//! over a folded project's tabs.

use gpui_kit::TestAppContext;
use koyo_agent::protocol::model::Status;

use super::{act, arc_shell, drawn, hear, unseen_chat};
use crate::arc::PreviousTab;
use crate::settings;
use crate::shell::key_of;
use crate::test_support::{click, draw};

#[gpui_kit::test]
fn the_projects_name_folds_its_worktrees_away_and_says_how_it_is_doing(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["arc-project-fold"], cx);
    let root = roots[0].clone();
    let pane = unseen_chat(&shell, cx);
    let key = key_of(&root);
    let folded = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).is_folded(&root));
    let says = |name: &str, cx: &mut TestAppContext| {
        draw(window, cx);
        let tab = drawn(window, format!("arc-tab-{name}-{pane}"), cx).is_some();
        let project = drawn(window, format!("arc-project-{name}-{key}"), cx).is_some();
        (tab, project)
    };
    hear(&shell, ("0001", &root), (Status::Working, None, false), cx);
    assert_eq!(says("working", cx), (true, false), "open: the tab's row says it");

    click(window, "arc-project-name", cx);
    assert!(folded(cx));
    assert!(drawn(window, "arc-header-0", cx).is_none(), "its worktrees folded away");
    assert_eq!(says("working", cx), (false, true), "folded: the name says it");
    hear(&shell, ("0001", &root), (Status::Idle, None, true), cx);
    assert_eq!(says("waiting", cx), (false, true));

    click(window, "arc-project-name", cx);
    assert!(!folded(cx));
    assert!(drawn(window, "arc-header-0", cx).is_some(), "its worktrees back");
    assert_eq!(says("waiting", cx), (true, false));
}

#[gpui_kit::test]
fn option_up_passes_over_a_folded_projects_tabs(cx: &mut TestAppContext) {
    let (window, shell, _) = arc_shell(&["arc-project-fold-keys"], cx);
    unseen_chat(&shell, cx);
    draw(window, cx);
    let shown = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.shown_tab());
    assert_eq!(shown(cx), Some((0, 0, 1)));

    click(window, "arc-project-name", cx);
    act(window, PreviousTab, cx);
    assert_eq!(shown(cx), Some((0, 0, 1)), "folded: no row of its to go to");

    click(window, "arc-project-name", cx);
    act(window, PreviousTab, cx);
    assert_eq!(shown(cx), Some((0, 0, 0)), "open: the row above");
}

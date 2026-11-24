//! ⌃⇥ across the window (`docs/tab-switcher.md`): the tabs looked at last, whichever project,
//! space or loose page they're in, and the one picked shown where it is.

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

use super::{arc_shell, chat, shown};
use crate::files::tests::{let_go, press};
use crate::model::tests::loose;
use crate::settings::{self, LoosePages};
use crate::shell::Shell;
use crate::test_support::draw;

/// The title of the tab the panes show, whether it's a loose page, and the space shown.
fn showing(shell: &Entity<Shell>, cx: &TestAppContext) -> (String, bool, usize) {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let title = (workspace.tab_shown()).map(|tab| tab.first().title.to_string());
        let loose = workspace.loose_shown().is_some();
        (title.unwrap_or_default(), loose, settings::get(cx).space)
    })
}

/// ⌃⇥ pressed `times`, then ⌃ let go.
fn switch(window: AnyWindowHandle, times: usize, cx: &mut TestAppContext) {
    for _ in 0..times {
        press(window, "ctrl-tab", cx);
    }
    let_go(window, cx);
    draw(window, cx);
}

#[gpui_kit::test]
fn ctrl_tab_goes_back_across_projects_spaces_and_loose_pages(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["switch-app", "switch-site"], cx);
    cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.loose_pages = LoosePages::PerSpace;
            let to = settings.add_space();
            settings.move_project(&roots[1], to);
        });
    });
    let change = |cx: &mut TestAppContext,
                  change: &dyn Fn(&mut Shell, &mut gpui_kit::Context<Shell>)| {
        shell.update(cx, |shell, cx| {
            change(shell, cx);
            cx.notify();
        });
        draw(window, cx);
    };
    change(cx, &|shell, _| shell.workspace.push_tab(chat("App chat")));
    change(cx, &|shell, cx| shell.show_space(1, cx));
    change(cx, &|shell, _| shell.workspace.push_tab(chat("Site chat")));
    change(cx, &|shell, _| {
        shell.workspace.add_loose(loose("Notes"), 0, true);
    });
    change(cx, &|shell, cx| shell.show_space(0, cx));
    assert_eq!(showing(&shell, cx), ("App chat".into(), false, 0));

    switch(window, 1, cx);
    assert_eq!(showing(&shell, cx), ("Notes".into(), true, 1));
    switch(window, 1, cx);
    assert_eq!(showing(&shell, cx), ("App chat".into(), false, 0));
    switch(window, 2, cx);
    assert_eq!(showing(&shell, cx), ("Site chat".into(), false, 1));
    assert_eq!(shown(&shell, cx), (roots[1].clone(), 1));
}

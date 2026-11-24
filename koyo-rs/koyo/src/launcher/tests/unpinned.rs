//! The launcher for an unpinned page only (`docs/layout.md`): the + after the unpinned pages
//! opens it whatever tab is shown, and ⌘T wherever the person is in no branch, a pinned or an
//! unpinned page shown. The field is alone, with no kind to pick, Tab goes nowhere, and Enter
//! opens an unpinned page in the space shown, and shows it. ⌘T on a branch's tab offers every
//! kind.

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

use super::{Kind, Launcher, kind, launch};
use crate::files::tests::{dialog, press};
use crate::shell::Shell;
use crate::test_support::{click, draw, drawn, loose, open_page, orca_on_chat, pin};

/// What the launcher's field says with nothing in it.
fn placeholder(launcher: &Entity<Launcher>, cx: &TestAppContext) -> String {
    cx.read(|cx| (launcher.read(cx).input.read(cx).presentation().placeholder()).to_string())
}

/// Whether the launcher in `window` offers kinds to pick: its pill, and the kind Tab goes to.
fn offers_kinds(window: AnyWindowHandle, cx: &mut TestAppContext) -> (bool, bool) {
    let pill = drawn(window, "launcher-kind", cx).is_some();
    (pill, drawn(window, "launcher-next-kind", cx).is_some())
}

/// The addresses of the unpinned pages the space shown lists, and the one shown.
fn unpinned(shell: &Entity<Shell>, cx: &TestAppContext) -> (Vec<String>, Option<usize>) {
    let shown = cx.read(|cx| shell.read(cx).workspace.loose_shown());
    (loose(shell, cx), shown)
}

/// The number of tabs the selected branch has.
fn branch_tabs(shell: &Entity<Shell>, cx: &TestAppContext) -> usize {
    cx.read(|cx| (shell.read(cx).workspace.branch()).map_or(0, |unit| unit.tabs.len()))
}

/// Types `address` in the launcher in `window` and presses Enter.
fn visit(window: AnyWindowHandle, address: &str, cx: &mut TestAppContext) {
    cx.simulate_input(window, address);
    press(window, "enter", cx);
    cx.run_until_parked();
}

/// The + after the unpinned pages opens the launcher for one while a branch's tab is shown: no
/// pill and no other kind, "Search or enter address", Tab going nowhere; Enter opens the address
/// typed as an unpinned page, shown, and the branch's tabs stay as they were.
#[gpui_kit::test]
fn the_plus_after_the_unpinned_pages_opens_one_while_a_branchs_tab_shows(
    cx: &mut TestAppContext,
) {
    let (window, shell) = orca_on_chat("launcher-unpinned-plus", cx);
    click(window, "orca-new-page", cx);
    cx.run_until_parked();
    let (launcher_window, launcher) = dialog::<Launcher>(cx).expect("the launcher");
    assert_eq!(offers_kinds(launcher_window, cx), (false, false), "the field alone");
    assert_eq!(placeholder(&launcher, cx), "Search or enter address");
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Browser, "Tab changes nothing");
    assert_eq!(offers_kinds(launcher_window, cx), (false, false));

    visit(launcher_window, "example.org/news", cx);
    assert!(dialog::<Launcher>(cx).is_none(), "it closes once it's done");
    let news = vec!["https://example.org/news".to_owned()];
    assert_eq!(unpinned(&shell, cx), (news, Some(0)), "an unpinned page, shown");
    assert_eq!(branch_tabs(&shell, cx), 1, "the branch's chat alone");
}

/// ⌘T on an unpinned or a pinned page opens the launcher for an unpinned page, and Enter opens
/// one after the one shown; ⌘T on a branch's tab opens it for any kind of tab.
#[gpui_kit::test]
fn cmd_t_on_an_unpinned_or_pinned_page_opens_it_for_an_unpinned_page_only(
    cx: &mut TestAppContext,
) {
    let (window, shell) = orca_on_chat("launcher-unpinned-cmd-t", cx);
    let page = open_page(window, &shell, Some("https://news.example/"), cx);
    shell.update(cx, |shell, cx| shell.loosen_tab_of(page, usize::MAX, cx));
    draw(window, cx);
    let (launcher_window, launcher) = launch(window, cx);
    assert_eq!(offers_kinds(launcher_window, cx), (false, false), "on an unpinned page");
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Browser);
    visit(launcher_window, "example.org/maps", cx);
    let pages = ["https://news.example/", "https://example.org/maps"].map(str::to_owned);
    assert_eq!(unpinned(&shell, cx), (pages.to_vec(), Some(1)));
    assert_eq!(branch_tabs(&shell, cx), 1);

    pin(window, &shell, &["https://pinned.example/"], cx);
    click(window, "orca-pinned-0", cx);
    let (launcher_window, _) = launch(window, cx);
    assert_eq!(offers_kinds(launcher_window, cx), (false, false), "on a pinned page");
    press(launcher_window, "escape", cx);
    cx.run_until_parked();

    click(window, "tab-0-0-0", cx);
    let (launcher_window, launcher) = launch(window, cx);
    assert_eq!(offers_kinds(launcher_window, cx), (true, true), "on a branch's tab");
    press(launcher_window, "tab", cx);
    assert_eq!(kind(&launcher, cx), Kind::Chat);
}

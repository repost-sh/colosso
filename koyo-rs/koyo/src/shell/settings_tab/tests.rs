//! The Settings tab (`docs/arc-sidebar.md` 11.9): ⌘, opens it among the loose tabs, or picks
//! it again; the workspace sidebar stays beside it in either layout, its own pages listed inside
//! it at its left, with no arrow back; its pages switch inside it, Back and Forward walking
//! them; and going to another tab or a worktree closes it, as its × does.

use gpui_kit::assets::IconName;
use gpui_kit::{AnyWindowHandle, Bounds, Entity, Pixels, TestAppContext, px};

use crate::files::tests::press;
use crate::model::{PaneKind, SettingsTab};
use crate::navigation::SettingsPage;
use crate::settings::Layout;
use crate::shell::Shell;
use crate::test_support::{click, draw, drawn, open_page, shell_on};
use crate::theme::TITLE_BAR_HEIGHT;

/// The shell in `layout` on a checkout whose one tab is a chat, motion reduced so what moves is
/// where it goes at once.
fn chat_shell(name: &str, layout: Layout, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    let (window, shell, _) = shell_on(name, layout, cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Chat);
        cx.notify();
    });
    draw(window, cx);
    (window, shell)
}

/// The Settings tab's row in `layout`'s list of loose tabs, and its ×.
fn tab_of(layout: Layout) -> (&'static str, &'static str) {
    match layout {
        Layout::Orca => ("orca-settings-tab", "orca-settings-tab-close"),
        Layout::Arc => ("arc-settings-tab", "arc-settings-tab-close"),
    }
}

/// Where the Settings tab's row is drawn in `window`, if it is.
fn settings_tab(window: AnyWindowHandle, layout: Layout, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
    drawn(window, tab_of(layout).0, cx)
}

/// Opens Settings as ⌘, does.
fn open_settings(window: AnyWindowHandle, cx: &mut TestAppContext) {
    press(window, "cmd-,", cx);
    draw(window, cx);
}

/// Whether the General page shows: its arrow-repeat switch is drawn.
fn on_general(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    drawn(window, "arrow-repeat", cx).is_some()
}

/// The link to the Appearance page in Settings' own list.
const APPEARANCE: &str = "settings-page-link-5";

#[gpui_kit::test]
fn settings_opens_as_one_loose_tab_in_the_orca_rail_and_opening_it_again_picks_it(
    cx: &mut TestAppContext,
) {
    let (window, shell) = chat_shell("settings-tab-orca-open", Layout::Orca, cx);
    assert!(settings_tab(window, Layout::Orca, cx).is_none());
    open_settings(window, cx);
    let tab = settings_tab(window, Layout::Orca, cx).expect("the Settings tab in the rail");
    let page = |cx: &TestAppContext| {
        cx.read(|cx| shell.read(cx).workspace.settings().map(SettingsTab::page))
    };
    assert_eq!(page(cx), Some(SettingsPage::General));
    assert_eq!(
        (SettingsTab::ICON, SettingsTab::TITLE),
        (IconName::Settings, "Settings"),
        "the icon pack's gear, where a page's own icon goes"
    );
    let hairline = drawn(window, "orca-rail-separator", cx).expect("the rail's hairline");
    let chat = drawn(window, "tab-0-0-0", cx).expect("the chat's tab");
    assert!(tab.top() >= px(0.) && tab.bottom() <= px(TITLE_BAR_HEIGHT));
    assert!(tab.right() <= hairline.left() && hairline.right() <= chat.left(), "{tab:?} {hairline:?} {chat:?}");

    click(window, APPEARANCE, cx);
    assert!(!on_general(window, cx), "Appearance shows");
    open_settings(window, cx);
    assert!(settings_tab(window, Layout::Orca, cx).is_some());
    assert!(!on_general(window, cx), "the tab open is picked, on the page it showed");
    assert_eq!(page(cx), Some(SettingsPage::Appearance), "still the one tab");
}

#[gpui_kit::test]
fn settings_opens_as_one_loose_tab_in_the_arc_sidebar(cx: &mut TestAppContext) {
    let (window, _shell) = chat_shell("settings-tab-arc-open", Layout::Arc, cx);
    open_settings(window, cx);
    let tab = settings_tab(window, Layout::Arc, cx).expect("the Settings tab in the sidebar");
    let sidebar = drawn(window, "arc-sidebar", cx).expect("the Arc sidebar");
    assert!(sidebar.contains(&tab.center()), "{tab:?} in {sidebar:?}");
    let project = drawn(window, "arc-project-name", cx).expect("the project's name");
    assert!(tab.bottom() <= project.top(), "over the open project, as loose tabs are");
}

#[gpui_kit::test]
fn the_workspace_sidebar_stays_beside_settings_in_the_orca_layout(cx: &mut TestAppContext) {
    let (window, _shell) = chat_shell("settings-tab-orca-sidebar", Layout::Orca, cx);
    open_settings(window, cx);
    let search = drawn(window, "sidebar-search", cx).expect("the workspace sidebar's search");
    let branch = drawn(window, "orca-branch-0-0", cx).expect("the worktree's row");
    let pages = drawn(window, "settings-page-link-0", cx).expect("Settings' own pages");
    assert!(pages.left() >= search.right().max(branch.right()), "{pages:?} right of the sidebar");
}

#[gpui_kit::test]
fn the_workspace_sidebar_stays_beside_settings_in_the_arc_layout(cx: &mut TestAppContext) {
    let (window, _shell) = chat_shell("settings-tab-arc-sidebar", Layout::Arc, cx);
    open_settings(window, cx);
    let sidebar = drawn(window, "arc-sidebar", cx).expect("the Arc sidebar");
    assert!(drawn(window, "tab-0-0-0", cx).is_some(), "the worktree's tabs are listed");
    let pages = drawn(window, "settings-page-link-0", cx).expect("Settings' own pages");
    assert!(pages.left() >= sidebar.right(), "{pages:?} right of {sidebar:?}");
}

#[gpui_kit::test]
fn settings_has_no_arrow_back(cx: &mut TestAppContext) {
    for (name, layout) in [("settings-tab-no-back-orca", Layout::Orca), ("settings-tab-no-back-arc", Layout::Arc)] {
        let (window, _shell) = chat_shell(name, layout, cx);
        open_settings(window, cx);
        assert!(settings_tab(window, layout, cx).is_some(), "{layout:?}");
        assert!(drawn(window, "settings-back", cx).is_none(), "{layout:?}");
    }
}

#[gpui_kit::test]
fn going_to_another_tab_or_a_worktree_closes_settings(cx: &mut TestAppContext) {
    let (window, _shell) = chat_shell("settings-tab-leave-orca", Layout::Orca, cx);
    open_settings(window, cx);
    assert!(settings_tab(window, Layout::Orca, cx).is_some());
    click(window, "tab-0-0-0", cx);
    assert!(settings_tab(window, Layout::Orca, cx).is_none(), "the chat's tab picked");
    assert!(drawn(window, "settings-page-link-0", cx).is_none());

    open_settings(window, cx);
    assert!(settings_tab(window, Layout::Orca, cx).is_some());
    click(window, "orca-branch-0-0", cx);
    assert!(settings_tab(window, Layout::Orca, cx).is_none(), "the worktree picked");

    let (window, _shell) = chat_shell("settings-tab-leave-arc", Layout::Arc, cx);
    open_settings(window, cx);
    assert!(settings_tab(window, Layout::Arc, cx).is_some());
    click(window, "tab-0-0-0", cx);
    assert!(settings_tab(window, Layout::Arc, cx).is_none(), "the chat's row picked");
}

#[gpui_kit::test]
fn the_close_button_closes_settings_and_what_it_covered_shows(cx: &mut TestAppContext) {
    for (name, layout) in [("settings-tab-close-orca", Layout::Orca), ("settings-tab-close-arc", Layout::Arc)] {
        let (window, shell) = chat_shell(name, layout, cx);
        open_settings(window, cx);
        assert!(settings_tab(window, layout, cx).is_some(), "{layout:?}");
        click(window, tab_of(layout).1, cx);
        assert!(settings_tab(window, layout, cx).is_none(), "{layout:?}");
        assert!(drawn(window, "settings-page-link-0", cx).is_none(), "{layout:?}");
        let chat = cx.read(|cx| {
            let tab = shell.read(cx).workspace.tab_shown();
            tab.map(|tab| tab.first().kind())
        });
        assert_eq!(chat, Some(PaneKind::Chat), "{layout:?}: the chat shows again");
    }
}

#[gpui_kit::test]
fn a_page_opened_while_settings_covers_a_loose_page_opens_as_a_loose_page(cx: &mut TestAppContext) {
    let (window, shell) = chat_shell("settings-tab-opens-loose", Layout::Orca, cx);
    open_page(window, &shell, Some("https://example.com/a"), cx);
    shell.update(cx, |shell, cx| {
        let at = shell.workspace.shown_tab().expect("the page's tab");
        assert!(shell.workspace.loosen_tab(at, usize::MAX));
        cx.notify();
    });
    draw(window, cx);
    open_settings(window, cx);
    open_page(window, &shell, Some("https://example.com/b"), cx);
    draw(window, cx);
    let opened = cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let tabs = workspace.branch().map(|unit| unit.tabs.len());
        (workspace.loose().len(), workspace.loose_shown(), tabs)
    });
    assert_eq!(opened, (2, Some(1), Some(1)), "after the loose page, the checkout's chat alone");
}

#[gpui_kit::test]
fn pages_switch_inside_the_settings_tab_and_back_and_forward_walk_them(cx: &mut TestAppContext) {
    let (window, _shell) = chat_shell("settings-tab-pages", Layout::Orca, cx);
    open_settings(window, cx);
    assert!(on_general(window, cx));
    let search = drawn(window, "sidebar-search", cx).expect("the workspace sidebar's search");
    let link = drawn(window, APPEARANCE, cx).expect("the link to Appearance");
    assert!(link.left() >= search.right(), "the link is inside the tab, right of the sidebar");
    click(window, APPEARANCE, cx);
    assert!(!on_general(window, cx), "Appearance shows");
    assert!(settings_tab(window, Layout::Orca, cx).is_some(), "in the same tab");
    press(window, "cmd-[", cx);
    assert!(on_general(window, cx), "Back to General");
    press(window, "cmd-]", cx);
    assert!(!on_general(window, cx), "Forward to Appearance");
    assert!(settings_tab(window, Layout::Orca, cx).is_some());
}

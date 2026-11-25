//! The Settings tab (`docs/sidebar.md` 11.9): ⌘, opens it among the loose tabs, or picks
//! it again; the workspace sidebar stays beside it wherever the tabs are, its own pages listed
//! inside it at its left, with no arrow back; its pages switch inside it, Back and Forward
//! walking them; it stays open behind another tab, a branch or a space shown, until its × or, while
//! it shows, ⌘W or Escape closes it.

use gpui_kit::assets::IconName;
use gpui_kit::{AnyWindowHandle, Bounds, Entity, Pixels, TestAppContext, px};

use crate::model::{PaneKind, SettingsTab};
use crate::navigation::SettingsPage;
use crate::settings::{self, Tabs};
use crate::shell::Shell;
use crate::shell::spaces::NewSpace;
use crate::test_support::{act, click, draw, drawn, open_page, press, shell_on};
use crate::theme::TITLE_BAR_HEIGHT;

/// The shell with the tabs where `tabs` puts them, on a checkout whose one tab is a chat, motion
/// reduced so what moves is where it goes at once.
fn chat_shell(name: &str, tabs: Tabs, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    let (window, shell, _) = shell_on(name, cx);
    cx.update(|cx| {
        settings::update(cx, |settings| settings.tabs = tabs);
        cx.set_reduce_motion(true);
    });
    shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Chat);
        cx.notify();
    });
    draw(window, cx);
    (window, shell)
}

/// The Settings tab's row among the loose tabs where `tabs` puts them, and its ×.
fn tab_of(tabs: Tabs) -> (&'static str, &'static str) {
    match tabs {
        Tabs::TopRail => ("rail-settings-tab", "rail-settings-tab-close"),
        Tabs::Sidebar => ("sidebar-settings-tab", "sidebar-settings-tab-close"),
    }
}

/// Where the Settings tab's row is drawn in `window`, if it is, with the tabs where `tabs` puts
/// them.
fn settings_tab(window: AnyWindowHandle, tabs: Tabs, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
    drawn(window, tab_of(tabs).0, cx)
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
fn settings_opens_as_one_loose_tab_in_the_top_rail_and_opening_it_again_picks_it(
    cx: &mut TestAppContext,
) {
    let (window, shell) = chat_shell("settings-tab-rail-open", Tabs::TopRail, cx);
    assert!(settings_tab(window, Tabs::TopRail, cx).is_none());
    open_settings(window, cx);
    let tab = settings_tab(window, Tabs::TopRail, cx).expect("the Settings tab in the rail");
    let page = |cx: &TestAppContext| {
        cx.read(|cx| shell.read(cx).workspace.settings().map(SettingsTab::page))
    };
    assert_eq!(page(cx), Some(SettingsPage::General));
    assert_eq!(
        (SettingsTab::ICON, SettingsTab::TITLE),
        (IconName::Settings, "Settings"),
        "the icon pack's gear, where a page's own icon goes"
    );
    let hairline = drawn(window, "rail-separator", cx).expect("the rail's hairline");
    let chat = drawn(window, "tab-0-0-0", cx).expect("the chat's tab");
    assert!(tab.top() >= px(0.) && tab.bottom() <= px(TITLE_BAR_HEIGHT));
    assert!(tab.right() <= hairline.left() && hairline.right() <= chat.left(), "{tab:?} {hairline:?} {chat:?}");

    click(window, APPEARANCE, cx);
    assert!(!on_general(window, cx), "Appearance shows");
    open_settings(window, cx);
    assert!(settings_tab(window, Tabs::TopRail, cx).is_some());
    assert!(!on_general(window, cx), "the tab open is picked, on the page it showed");
    assert_eq!(page(cx), Some(SettingsPage::Appearance), "still the one tab");
}

#[gpui_kit::test]
fn settings_opens_as_one_loose_tab_in_the_sidebar(cx: &mut TestAppContext) {
    let (window, _shell) = chat_shell("settings-tab-sidebar-open", Tabs::Sidebar, cx);
    open_settings(window, cx);
    let tab = settings_tab(window, Tabs::Sidebar, cx).expect("the Settings tab in the sidebar");
    let sidebar = drawn(window, "sidebar", cx).expect("the sidebar");
    assert!(sidebar.contains(&tab.center()), "{tab:?} in {sidebar:?}");
    let project = drawn(window, "sidebar-project-0", cx).expect("the project's row");
    assert!(tab.bottom() <= project.top(), "over the projects, as loose tabs are");
}

#[gpui_kit::test]
fn the_workspace_sidebar_stays_beside_settings_with_the_tabs_on_the_top_rail(
    cx: &mut TestAppContext,
) {
    let (window, _shell) = chat_shell("settings-tab-rail-sidebar", Tabs::TopRail, cx);
    open_settings(window, cx);
    let sidebar = drawn(window, "sidebar", cx).expect("the workspace sidebar");
    let branch = drawn(window, "sidebar-branch-0-0", cx).expect("the worktree's row");
    let pages = drawn(window, "settings-page-link-0", cx).expect("Settings' own pages");
    assert!(pages.left() >= sidebar.right().max(branch.right()), "{pages:?} right of the sidebar");
}

#[gpui_kit::test]
fn the_workspace_sidebar_stays_beside_settings_with_the_tabs_in_it(cx: &mut TestAppContext) {
    let (window, _shell) = chat_shell("settings-tab-sidebar-sidebar", Tabs::Sidebar, cx);
    open_settings(window, cx);
    let sidebar = drawn(window, "sidebar", cx).expect("the sidebar");
    assert!(drawn(window, "tab-0-0-0", cx).is_some(), "the worktree's tabs are listed");
    let pages = drawn(window, "settings-page-link-0", cx).expect("Settings' own pages");
    assert!(pages.left() >= sidebar.right(), "{pages:?} right of {sidebar:?}");
}

#[gpui_kit::test]
fn settings_has_no_arrow_back(cx: &mut TestAppContext) {
    for (name, tabs) in [("settings-tab-no-back-rail", Tabs::TopRail), ("settings-tab-no-back-sidebar", Tabs::Sidebar)] {
        let (window, _shell) = chat_shell(name, tabs, cx);
        open_settings(window, cx);
        assert!(settings_tab(window, tabs, cx).is_some(), "{tabs:?}");
        assert!(drawn(window, "settings-back", cx).is_none(), "{tabs:?}");
    }
}

/// Whether Settings' own pages are drawn: whether its tab is the one shown.
fn shows_settings(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    drawn(window, "settings-page-link-0", cx).is_some()
}

#[gpui_kit::test]
fn settings_stays_open_as_another_tab_shows_and_its_tab_picks_it_again(cx: &mut TestAppContext) {
    for (name, tabs) in [("settings-tab-stays-rail", Tabs::TopRail), ("settings-tab-stays-sidebar", Tabs::Sidebar)] {
        let (window, shell) = chat_shell(name, tabs, cx);
        open_settings(window, cx);
        click(window, APPEARANCE, cx);
        click(window, "tab-0-0-0", cx);
        assert!(settings_tab(window, tabs, cx).is_some(), "{tabs:?}: open behind the chat");
        assert!(!shows_settings(window, cx), "{tabs:?}: the chat shows");
        let kind = |cx: &TestAppContext| {
            cx.read(|cx| shell.read(cx).workspace.tab_shown().map(|tab| tab.first().kind()))
        };
        assert_eq!(kind(cx), Some(PaneKind::Chat), "{tabs:?}");

        click(window, tab_of(tabs).0, cx);
        assert!(shows_settings(window, cx), "{tabs:?}: picked, it shows");
        assert!(!on_general(window, cx), "{tabs:?}: on the page it showed");
        press(window, "cmd-[", cx);
        assert!(on_general(window, cx), "{tabs:?}: with the pages behind it");
    }
}

#[gpui_kit::test]
fn settings_stays_open_behind_a_branch_picked_and_a_space_shown(cx: &mut TestAppContext) {
    let (window, _shell) = chat_shell("settings-tab-stays-behind", Tabs::TopRail, cx);
    open_settings(window, cx);
    click(window, "sidebar-branch-0-0", cx);
    assert!(settings_tab(window, Tabs::TopRail, cx).is_some(), "behind the branch's tab");
    assert!(!shows_settings(window, cx));

    open_settings(window, cx);
    assert!(shows_settings(window, cx), "opening it again picks it");
    act(window, NewSpace, cx);
    assert!(settings_tab(window, Tabs::TopRail, cx).is_some(), "in the next space too");
    assert!(!shows_settings(window, cx));
}

#[gpui_kit::test]
fn cmd_w_and_escape_close_settings_only_while_it_shows_and_its_x_from_behind(cx: &mut TestAppContext) {
    let (window, shell) = chat_shell("settings-tab-keys", Tabs::TopRail, cx);
    let tabs = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.branch().map(|unit| unit.tabs.len()));
    open_settings(window, cx);
    press(window, "escape", cx);
    draw(window, cx);
    assert!(settings_tab(window, Tabs::TopRail, cx).is_none(), "Escape on it");

    open_settings(window, cx);
    click(window, "tab-0-0-0", cx);
    press(window, "escape", cx);
    draw(window, cx);
    assert!(settings_tab(window, Tabs::TopRail, cx).is_some(), "Escape on the chat leaves it");
    click(window, tab_of(Tabs::TopRail).1, cx);
    assert!(settings_tab(window, Tabs::TopRail, cx).is_none(), "its \u{d7} from behind");
    assert_eq!(tabs(cx), Some(1), "the chat stays");

    open_settings(window, cx);
    click(window, "tab-0-0-0", cx);
    press(window, "cmd-w", cx);
    draw(window, cx);
    assert_eq!(tabs(cx), Some(0), "\u{2318}W closes the chat shown");
    assert!(settings_tab(window, Tabs::TopRail, cx).is_some(), "and not Settings behind it");
}

#[gpui_kit::test]
fn the_close_button_closes_settings_and_what_it_covered_shows(cx: &mut TestAppContext) {
    for (name, tabs) in [("settings-tab-close-rail", Tabs::TopRail), ("settings-tab-close-sidebar", Tabs::Sidebar)] {
        let (window, shell) = chat_shell(name, tabs, cx);
        open_settings(window, cx);
        assert!(settings_tab(window, tabs, cx).is_some(), "{tabs:?}");
        click(window, tab_of(tabs).1, cx);
        assert!(settings_tab(window, tabs, cx).is_none(), "{tabs:?}");
        assert!(drawn(window, "settings-page-link-0", cx).is_none(), "{tabs:?}");
        let chat = cx.read(|cx| {
            let tab = shell.read(cx).workspace.tab_shown();
            tab.map(|tab| tab.first().kind())
        });
        assert_eq!(chat, Some(PaneKind::Chat), "{tabs:?}: the chat shows again");
    }
}

#[gpui_kit::test]
fn a_page_opened_while_settings_covers_a_loose_page_opens_as_a_loose_page(cx: &mut TestAppContext) {
    let (window, shell) = chat_shell("settings-tab-opens-loose", Tabs::TopRail, cx);
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
    let (window, _shell) = chat_shell("settings-tab-pages", Tabs::TopRail, cx);
    open_settings(window, cx);
    assert!(on_general(window, cx));
    let sidebar = drawn(window, "sidebar", cx).expect("the workspace sidebar");
    let link = drawn(window, APPEARANCE, cx).expect("the link to Appearance");
    assert!(link.left() >= sidebar.right(), "the link is inside the tab, right of the sidebar");
    click(window, APPEARANCE, cx);
    assert!(!on_general(window, cx), "Appearance shows");
    assert!(settings_tab(window, Tabs::TopRail, cx).is_some(), "in the same tab");
    press(window, "cmd-[", cx);
    assert!(on_general(window, cx), "Back to General");
    press(window, "cmd-]", cx);
    assert!(!on_general(window, cx), "Forward to Appearance");
    assert!(settings_tab(window, Tabs::TopRail, cx).is_some());
}

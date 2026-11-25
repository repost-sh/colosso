//! ⌘L, with the address bar in the sidebar, opens the launcher on the page the person is in:
//! its address in the field, all of it selected, and a page or a search picked loads in that
//! page. With the address bar in the page the keyboard goes to the page's own field instead.
//! With no page in the tab it opens nothing.

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, Focusable as _, TestAppContext, Window,
};

use super::{Kind, Launcher, kind, launch, launch_with, shell};
use crate::browser::BrowserView;
use crate::files::tests::{dialog, tabs};
use crate::history;
use crate::model::{Edge, Pane, PaneKind};
use crate::settings::{self, AddressBar};
use crate::shell::Shell;
use crate::test_support::{click_pane, draw, open_page, press};

const DOCS: &str = "https://www.example.com/docs?page=2";

/// Opens a browser tab at `url` on the selected checkout: its pane and its page.
fn open(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    url: &str,
    cx: &mut TestAppContext,
) -> (u64, Entity<BrowserView>) {
    let pane = open_page(window, shell, Some(url), cx);
    draw(window, cx);
    let view = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch()?;
        unit.pane(pane)?.view().cloned()
    });
    (pane, view.expect("its page"))
}

/// A shell with the address bar at `address_bar` over a checkout of its own, a browser tab at
/// [`DOCS`] shown: the window, the shell, and the tab's pane and page.
fn on_page(
    name: &str,
    address_bar: AddressBar,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, u64, Entity<BrowserView>) {
    let (window, shell, _) = shell(name, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.address_bar = address_bar));
    let (pane, view) = open(window, &shell, DOCS, cx);
    (window, shell, pane, view)
}

/// Presses ⌘L in `window`: the launcher's window and the launcher.
fn open_address(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Launcher>) {
    launch_with(window, "cmd-l", cx)
}

/// Words typed on a page are searched for with the engine the person picked, as the launcher's
/// search row says, in the page.
#[gpui_kit::test]
fn words_typed_on_a_page_are_searched_for_with_the_engine_picked(cx: &mut TestAppContext) {
    let (window, _, _, view) = on_page("launcher-search-engine", AddressBar::Sidebar, cx);
    let kagi = crate::browser::SearchEngine::Kagi;
    cx.update(|cx| settings::update(cx, |settings| settings.search_engine = kagi));
    let (launcher_window, launcher) = open_address(window, cx);
    cx.simulate_input(launcher_window, "rust gpui");
    let hint = cx.read(|cx| launcher.read(cx).rows[0].hint);
    assert_eq!(hint, Some("Search with Kagi"));
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    let address = cx.read(|cx| view.read(cx).address());
    assert_eq!(address, "https://kagi.com/search?q=rust+gpui");
}

/// What the launcher's field holds, and where what's selected in it starts and ends.
fn field(launcher: &Entity<Launcher>, cx: &TestAppContext) -> (String, (usize, usize)) {
    cx.read(|cx| {
        let input = launcher.read(cx).input.read(cx);
        let selected = input.selected_range();
        (input.value().to_string(), (selected.start, selected.end))
    })
}

fn in_window<T>(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
    read: impl FnOnce(&mut Window, &mut App) -> T,
) -> T {
    cx.update_window(window, |_, window, cx| read(window, cx))
        .unwrap()
}

#[gpui_kit::test]
fn cmd_l_opens_the_launcher_on_the_page_and_what_is_typed_loads_in_it(cx: &mut TestAppContext) {
    let (window, shell, pane, view) = on_page("launcher-address", AddressBar::Sidebar, cx);
    let (launcher_window, launcher) = open_address(window, cx);
    assert_eq!(kind(&launcher, cx), Kind::Browser);
    assert_eq!(
        field(&launcher, cx),
        (DOCS.to_owned(), (0, DOCS.len())),
        "the page's whole address, all of it selected"
    );

    cx.simulate_input(launcher_window, "example.org/next");
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_none(), "it closes once it's done");
    let address = |cx: &TestAppContext| cx.read(|cx| view.read(cx).address());
    assert_eq!(address(cx), "https://example.org/next");
    assert_eq!(tabs(&shell, cx).len(), 1, "in the page, not in a new tab");
    draw(window, cx);
    let keyboard = in_window(window, cx, |window, cx| {
        view.focus_handle(cx).is_focused(window)
    });
    assert!(keyboard, "the page takes the keyboard after");
    let focused = in_window(window, cx, |window, cx| {
        shell.read(cx).focused_pane(window, cx)
    });
    assert_eq!(focused, Some(pane));

    let (launcher_window, _) = open_address(window, cx);
    cx.simulate_input(launcher_window, "rust gpui");
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    assert_eq!(address(cx), "https://www.google.com/search?q=rust+gpui");
    let searched: Vec<String> = cx.read(|cx| {
        (history::get(cx).of(history::Kind::Search))
            .map(|entry| entry.text.clone())
            .collect()
    });
    assert_eq!(searched, ["rust gpui"]);
    assert_eq!(tabs(&shell, cx).len(), 1);

    let (launcher_window, _) = open_address(window, cx);
    press(launcher_window, "backspace enter", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_some(), "no address, no visit");
    assert_eq!(tabs(&shell, cx).len(), 1);
    press(launcher_window, "tab enter", cx);
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx).len(), 2, "a new chat");
}

/// What ⌘T's launcher starts with, dismissed again as it was. No launcher is held on to past
/// its window: one that's held isn't gone, and so says nothing of what was typed in it.
fn offered(window: AnyWindowHandle, cx: &mut TestAppContext) -> (String, Kind) {
    let (launcher_window, launcher) = launch(window, cx);
    let offered = (field(&launcher, cx).0, kind(&launcher, cx));
    drop(launcher);
    press(launcher_window, "escape", cx);
    cx.run_until_parked();
    offered
}

#[gpui_kit::test]
fn the_address_is_not_text_dismissed_nor_replaced_by_it(cx: &mut TestAppContext) {
    let (window, _, _, _) = on_page("launcher-address-dismissed", AddressBar::Sidebar, cx);
    let (launcher_window, _) = open_address(window, cx);
    press(launcher_window, "escape", cx);
    cx.run_until_parked();
    assert_eq!(
        offered(window, cx),
        (String::new(), Kind::Browser),
        "nobody typed the address"
    );

    let (launcher_window, _) = launch(window, cx);
    press(launcher_window, "tab", cx);
    cx.simulate_input(launcher_window, "half a thought");
    press(launcher_window, "escape", cx);
    cx.run_until_parked();
    let (launcher_window, launcher) = open_address(window, cx);
    assert_eq!(kind(&launcher, cx), Kind::Browser);
    assert_eq!(field(&launcher, cx).0, DOCS);
    press(launcher_window, "end", cx);
    assert_eq!(field(&launcher, cx).1, (DOCS.len(), DOCS.len()));
    press(launcher_window, "cmd-l", cx);
    assert_eq!(field(&launcher, cx).1, (0, DOCS.len()));
    drop(launcher);
    press(launcher_window, "escape", cx);
    cx.run_until_parked();
    let thought = ("half a thought".to_owned(), Kind::Chat);
    assert_eq!(offered(window, cx), thought);

    let (launcher_window, _) = open_address(window, cx);
    cx.simulate_input(launcher_window, "example.org");
    press(launcher_window, "escape", cx);
    cx.run_until_parked();
    assert_eq!(
        offered(window, cx),
        ("example.org".to_owned(), Kind::Browser)
    );
}

#[gpui_kit::test]
fn with_no_page_in_the_tab_cmd_l_opens_nothing(cx: &mut TestAppContext) {
    let (window, shell, _, view) = on_page("launcher-address-none", AddressBar::Sidebar, cx);
    shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
    for address_bar in [AddressBar::Sidebar, AddressBar::Page] {
        cx.update(|cx| settings::update(cx, |settings| settings.address_bar = address_bar));
        draw(window, cx);
        draw(window, cx);
        press(window, "cmd-l", cx);
        cx.run_until_parked();
        assert!(dialog::<Launcher>(cx).is_none(), "no address to change");
        assert_eq!(tabs(&shell, cx).len(), 2, "and no tab opened for one");
        let editing = in_window(window, cx, |window, cx| {
            let field = view.read(cx).address_field().read(cx);
            field.focus_handle(cx).is_focused(window)
        });
        assert!(!editing, "the other tab's page is left alone");
    }
}

#[gpui_kit::test]
fn in_a_split_cmd_l_opens_on_the_page_the_person_is_in(cx: &mut TestAppContext) {
    let (window, shell, first, _) = on_page("launcher-address-split", AddressBar::Sidebar, cx);
    let (second, _) = open(window, &shell, "https://second.example/", cx);
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout");
        assert!(
            unit.drop_tab(1, first, Edge::Right),
            "two pages side by side"
        );
        cx.notify();
    });
    let opens_on = |cx: &mut TestAppContext| {
        let (launcher_window, launcher) = open_address(window, cx);
        let address = field(&launcher, cx).0;
        press(launcher_window, "escape", cx);
        cx.run_until_parked();
        address
    };
    click_pane(window, second, cx);
    assert_eq!(opens_on(cx), "https://second.example/");
    click_pane(window, first, cx);
    assert_eq!(opens_on(cx), DOCS);

    let (launcher_window, _) = open_address(window, cx);
    cx.simulate_input(launcher_window, "example.org");
    press(launcher_window, "enter", cx);
    cx.run_until_parked();
    let addresses: Vec<String> = cx.read(|cx| {
        let tab = shell.read(cx).workspace.tab_shown().expect("the split");
        (tab.panes().into_iter())
            .filter_map(Pane::address)
            .map(str::to_owned)
            .collect()
    });
    assert_eq!(
        addresses,
        ["https://example.org", "https://second.example/"]
    );
}

/// With the address bar in the page, ⌘L from anywhere in the tab puts the
/// keyboard in the page's own field, its whole address there, and opens no launcher.
#[gpui_kit::test]
fn with_the_address_bar_in_the_page_cmd_l_puts_the_keyboard_in_the_pages_own_field(
    cx: &mut TestAppContext,
) {
    let (window, shell, _, view) = on_page("launcher-address-in-page", AddressBar::Page, cx);
    shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
    draw(window, cx);
    let chat = cx.read(|cx| {
        shell
            .read(cx)
            .workspace
            .tab_shown()
            .map(|tab| tab.first().id)
    });
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout");
        let beside = unit.tabs[0].first().id;
        assert!(unit.drop_tab(1, beside, Edge::Left));
        cx.notify();
    });
    draw(window, cx);
    click_pane(window, chat.expect("the chat's pane"), cx);
    press(window, "cmd-l", cx);
    cx.run_until_parked();
    assert!(
        dialog::<Launcher>(cx).is_none(),
        "no launcher, the page has a field"
    );
    let (editing, value) = in_window(window, cx, |window, cx| {
        let field = view.read(cx).address_field().read(cx);
        (
            field.focus_handle(cx).is_focused(window),
            field.value().to_string(),
        )
    });
    assert!(editing, "the keyboard is in the page's address");
    assert_eq!(value, DOCS);
}

//! Each kind of pane takes the keyboard into the view it shows: the agent desktop's, a device's,
//! a terminal's and a file brought back when asked for, the first of a tab's panes by their kinds
//! when the tab is shown; and the desktop's tab and a device's come back with ⌘⇧T showing what
//! they showed.

use gpui_kit::TestAppContext;

use super::{ask_for, shell, typing_in};
use crate::files::Place;
use crate::files::tests::with_shell;
use crate::model::{Edge, Pane, PaneKind, Tab};
use crate::settings::Layout;
use crate::terminals::budget::no_daemon;
use crate::test_support::{
    close_shown, draw, iphone, keyboard_in, open_page, shell_on, terminals_listed,
};

#[gpui_kit::test]
fn the_desktop_and_a_device_take_the_keyboard_and_come_back_as_they_were(cx: &mut TestAppContext) {
    let (window, shell) = shell("keys-desktop-device", cx);
    let (desktop, device) = shell.update(cx, |shell, cx| {
        let desktop = shell.desktop_pane(false, cx).expect("the desktop's tab");
        let device = shell
            .device_pane(Some(iphone("UDID-KEYS")), false, cx)
            .expect("the device's tab");
        (desktop, device)
    });
    for (pane, context, title, udid) in [
        (desktop, "AgentDesktop", "Agent desktop", None),
        (device, "Device", "iPhone 17 Pro", Some("UDID-KEYS")),
    ] {
        ask_for(window, &shell, pane, cx);
        assert!(keyboard_in(window, context, cx), "{title}, asked for");
        close_shown(window, &shell, cx);
        assert!(!keyboard_in(window, context, cx), "{title}, closed");
        cx.simulate_keystrokes(window, "cmd-shift-t");
        assert!(keyboard_in(window, context, cx), "{title}, back with ⌘⇧T");
        let back = shell.read_with(cx, |shell, cx| {
            let pane = shell.workspace.tab_shown().expect("the tab back").first();
            let device = pane.device_view().and_then(|view| view.read(cx).chosen().cloned());
            let bar = pane.bar(cx).is_some();
            (pane.title.to_string(), device.map(|device| device.udid), bar)
        });
        let showed = (title.to_owned(), udid.map(str::to_owned), udid.is_some());
        assert_eq!(back, showed, "showing what it showed, a device its bar");
    }
}

/// A terminal's tab not shown yet, asked for, makes the terminal's view and gives it the
/// keyboard.
#[gpui_kit::test]
fn a_terminal_asked_for_takes_the_keyboard_into_its_view(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("keys-terminal", Layout::default(), cx);
    no_daemon(&shell, cx);
    terminals_listed(&shell, &root, &[("term_keys", "zsh")], cx);
    let terminal = shell.read_with(cx, |shell, _| {
        let found = shell.workspace.find_pane(|pane| pane.terminal().is_some());
        found.map(|(_, pane)| pane).expect("the terminal's tab")
    });
    draw(window, cx);
    assert!(!keyboard_in(window, koyo_term::CONTEXT, cx), "not shown yet");
    ask_for(window, &shell, terminal, cx);
    assert!(keyboard_in(window, koyo_term::CONTEXT, cx), "the terminal's view");
}

/// A tab shown hands the keyboard to the pane the person was in, a file only once they've
/// been in it: beside a file only looked at, a device takes it.
#[gpui_kit::test]
fn a_device_beside_a_file_only_looked_at_takes_the_keyboard(cx: &mut TestAppContext) {
    let (window, shell) = shell("keys-file-device", cx);
    with_shell(window, &shell, cx, |shell, window, cx| {
        shell.open_file(Place::file("README.md"), true, false, window, cx);
        shell.device_pane(Some(iphone("UDID-KEYS")), false, cx);
        let unit = shell.workspace.branch_mut().expect("a checkout");
        let file = unit.tabs[0].first().id;
        assert!(unit.drop_tab(1, file, Edge::Right), "the device beside the file");
        shell.new_tab(PaneKind::Chat, cx);
    });
    draw(window, cx);
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 0, cx));
    assert!(keyboard_in(window, "Device", cx), "the device, not the file looked at");
}

/// Of a tab's chat and page beside a file only looked at, the chat comes first, and takes the
/// keyboard only when just opened: the page after it doesn't take it either.
#[gpui_kit::test]
fn a_chat_before_a_page_keeps_the_keyboard_from_it_when_its_tab_is_shown(
    cx: &mut TestAppContext,
) {
    let (window, shell) = shell("keys-chat-page", cx);
    with_shell(window, &shell, cx, |shell, window, cx| {
        shell.open_file(Place::file("README.md"), true, false, window, cx);
        shell.new_tab(PaneKind::Chat, cx);
    });
    draw(window, cx);
    let page = open_page(window, &shell, Some("https://example.com/"), cx);
    draw(window, cx);
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout");
        let (file, chat) = (unit.tabs[0].first().id, unit.tabs[1].first().id);
        let page_at = unit.tab_with(page).expect("the page's tab");
        assert!(unit.drop_tab(page_at, chat, Edge::Right), "the page beside the chat");
        assert!(unit.drop_tab(1, file, Edge::Right), "the two beside the file");
        shell.new_tab(PaneKind::Chat, cx);
    });
    draw(window, cx);
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 0, cx));
    let typing = typing_in(window, &shell, cx);
    assert!(typing != Some(page), "the page took it: {typing:?}");
}

/// A file the window brought back, asked for before its tab was shown, opens and takes the
/// keyboard, as a file open already does.
#[gpui_kit::test]
fn a_file_brought_back_and_asked_for_opens_with_the_keyboard(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("keys-file-back", Layout::default(), cx);
    let file = shell.update(cx, |shell, cx| {
        shell.new_tab(PaneKind::Chat, cx);
        let pane = Pane::of_path(root.join("README.md"));
        let id = pane.id;
        shell.workspace.branch_mut().expect("a checkout").tabs.push(Tab::Single(pane));
        id
    });
    draw(window, cx);
    ask_for(window, &shell, file, cx);
    assert_eq!(typing_in(window, &shell, cx), Some(file), "the file, opened");
}

//! The downloads button in the window's top bar, beside the sidebar's toggle, as Safari's is:
//! there once there are downloads to list, never in a page's address field, ⌥⌘L or a press
//! opening the list under it, and nothing opening by itself as a download starts.

use gpui_kit::{AnyWindowHandle, Bounds, Pixels, TestAppContext, VisualTestContext};
use gpui_kit::Entity;
use koyo_browser_ipc::DownloadState;

use super::*;
use crate::popup::{self, Kind};
use crate::settings::Layout;
use crate::shell::Shell;
use crate::test_support::{click, draw, open_page_view, shell_on};

/// The shell in `layout`, a host of the test's own, and the list's folder.
fn window_with_host(name: &str, layout: Layout, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>, FakeHost, PathBuf) {
    let (window, shell, _) = shell_on(name, layout, cx);
    let host = FakeHost::serve(&format!("{name}-host"), cx);
    let folder = crate::test_support::scratch(&format!("{name}-downloads"));
    let downloads = cx.update(Downloads::global);
    downloads.update(cx, |downloads, _| downloads.folder = Some(folder.clone()));
    (window, shell, host, folder)
}

fn drawn(window: AnyWindowHandle, selector: &'static str, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
    draw(window, cx);
    VisualTestContext::from_window(window, cx).debug_bounds(selector)
}

/// The list's window, while it's up.
fn list_up(cx: &mut TestAppContext) -> Option<AnyWindowHandle> {
    let menu = cx.read(|cx| popup::window(Kind::Menu, cx))?;
    drawn(menu, "downloads-panel", cx).map(|_| menu)
}

fn top_bar_has_the_button_once_there_are_downloads(name: &str, layout: Layout, cx: &mut TestAppContext) {
    let (window, _, _host, _) = window_with_host(name, layout, cx);
    assert_eq!(drawn(window, "downloads-button", cx), None, "no downloads, no button");
    asked(1, At::Person, cx);
    let button = drawn(window, "downloads-button", cx).expect("the button, with a download listed");
    let toggle = drawn(window, "toggle-sidebar", cx).expect("the sidebar's toggle");
    assert!(
        button.bottom() <= gpui_kit::px(crate::theme::TITLE_BAR_HEIGHT),
        "in the top bar: {button:?}"
    );
    assert!(
        button.left() >= toggle.right() && button.left() - toggle.right() < gpui_kit::px(12.),
        "right beside the sidebar's toggle: {button:?}, {toggle:?}"
    );
}

#[gpui_kit::test]
fn the_arc_top_bar_has_the_downloads_button_once_there_are_downloads(cx: &mut TestAppContext) {
    top_bar_has_the_button_once_there_are_downloads("downloads-top-arc", Layout::Arc, cx);
}

#[gpui_kit::test]
fn the_orca_top_bar_has_the_downloads_button_once_there_are_downloads(cx: &mut TestAppContext) {
    top_bar_has_the_button_once_there_are_downloads("downloads-top-orca", Layout::Orca, cx);
}

fn the_address_field_has_no_downloads_button(name: &str, layout: Layout, field: &'static str, cx: &mut TestAppContext) {
    let (window, shell, _host, _) = window_with_host(name, layout, cx);
    open_page_view(window, &shell, "https://example.com/", cx);
    asked(1, At::Person, cx);
    let field = drawn(window, field, cx).expect("the page's address field");
    let button = drawn(window, "downloads-button", cx).expect("the button");
    assert!(!field.intersects(&button), "the field {field:?}, the button {button:?}");
}

#[gpui_kit::test]
fn the_arc_address_bar_has_no_downloads_button(cx: &mut TestAppContext) {
    the_address_field_has_no_downloads_button("downloads-field-arc", Layout::Arc, "arc-address", cx);
}

#[gpui_kit::test]
fn a_pages_toolbar_has_no_downloads_button(cx: &mut TestAppContext) {
    the_address_field_has_no_downloads_button("downloads-field-orca", Layout::Orca, "browser-address", cx);
}

#[gpui_kit::test]
fn a_download_starting_or_ending_opens_nothing(cx: &mut TestAppContext) {
    let (window, _, host, folder) = window_with_host("downloads-open-nothing", Layout::Arc, cx);
    asked(1, At::Person, cx);
    draw(window, cx);
    host.says(going(1, &folder.join("report.pdf"), 512, DownloadState::Going), cx);
    draw(window, cx);
    assert_eq!(list_up(cx), None, "nothing opens as it starts");
    host.says(going(1, &folder.join("report.pdf"), 2048, DownloadState::Done), cx);
    draw(window, cx);
    assert_eq!(list_up(cx), None, "nor as it ends");
}

#[gpui_kit::test]
fn option_command_l_toggles_the_list(cx: &mut TestAppContext) {
    let (window, _, _host, _) = window_with_host("downloads-key", Layout::Orca, cx);
    cx.simulate_keystrokes(window, "alt-cmd-l");
    assert_eq!(list_up(cx), None, "with nothing to list it opens nothing");
    asked(1, At::Person, cx);
    draw(window, cx);
    cx.simulate_keystrokes(window, "alt-cmd-l");
    let list = list_up(cx).expect("⌥⌘L opens the list");
    let button = drawn(window, "downloads-button", cx).expect("the button");
    let bounds = |handle: AnyWindowHandle, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, _| window.bounds()).unwrap()
    };
    let (main, under) = (bounds(window, cx), bounds(list, cx));
    let middle = main.origin.x + button.center().x;
    assert!(
        under.left() < middle && middle < under.right() && under.top() >= main.origin.y + button.bottom(),
        "the list opens under its button: {under:?}, the button {button:?} in {main:?}"
    );
    cx.simulate_keystrokes(window, "alt-cmd-l");
    cx.run_until_parked();
    assert_eq!(list_up(cx), None, "and again closes it");
}

#[gpui_kit::test]
fn the_list_shows_each_download_as_it_stands_with_what_can_be_done_to_it(cx: &mut TestAppContext) {
    let (window, _, host, folder) = window_with_host("downloads-rows", Layout::Arc, cx);
    for id in 1..=4 {
        asked(id, At::Person, cx);
    }
    let path = |n: u32| folder.join(format!("report ({n}).pdf"));
    host.says(going(1, &path(1), 512, DownloadState::Going), cx);
    host.says(going(2, &path(2), 2048, DownloadState::Done), cx);
    let reason = "The network went away.".to_owned();
    host.says(going(3, &path(3), 10, DownloadState::Failed { reason }), cx);
    host.says(going(4, &path(4), 10, DownloadState::Canceled), cx);
    draw(window, cx);
    click(window, "downloads-button", cx);
    let list = list_up(cx).expect("a press on the button opens the list");
    cx.update(|cx| {
        let said: Vec<String> = Downloads::listed(cx).iter().map(panel::status).collect();
        assert_eq!(said, ["512 bytes of 2.0 KB", "2.0 KB · Done", "Failed · The network went away.", "Canceled"]);
    });
    let has = |selector: &'static str, cx: &mut TestAppContext| drawn(list, selector, cx).is_some();
    let offered: Vec<(bool, bool)> = [
        ("download-reveal-0", "download-cancel-0"),
        ("download-reveal-1", "download-cancel-1"),
        ("download-reveal-2", "download-cancel-2"),
        ("download-reveal-3", "download-cancel-3"),
    ]
    .into_iter()
    .map(|(reveal, cancel)| (has(reveal, cx), has(cancel, cx)))
    .collect();
    assert_eq!(
        offered,
        [(true, true), (true, false), (false, false), (false, false)],
        "Show in Finder for a file there, Cancel while it comes in"
    );
    host.asked(cx);
    click(list, "download-cancel-0", cx);
    assert_eq!(acts(&host, cx), [(1, DownloadAct::Cancel)]);
    assert!(list_up(cx).is_some(), "canceling one leaves the list up");
    let clear = crate::test_support::center_of(list, "downloads-clear", cx);
    VisualTestContext::from_window(list, cx).simulate_click(clear, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(stages(cx), [], "Clear takes off every one that ended");
    assert_eq!(list_up(cx), None, "and with none left the list goes");
    assert_eq!(drawn(window, "downloads-button", cx), None, "and its button");
}

#[test]
fn a_size_under_a_kilobyte_and_a_half_is_said_in_kilobytes() {
    assert_eq!(size(1280), "1.3 KB", "Koyo never says 1,280 B: that's Chrome's own download bubble");
}

#[gpui_kit::test]
fn an_agents_download_is_listed_with_its_agent(cx: &mut TestAppContext) {
    let (window, _, _host, _) = window_with_host("downloads-agent", Layout::Arc, cx);
    asked(1, At::Agent("claude-code:7f3a".into()), cx);
    assert!(drawn(window, "downloads-button", cx).is_some());
    assert_eq!(list_up(cx), None, "an agent's download opens nothing either");
    cx.update(|cx| assert!(panel::status(&Downloads::listed(cx)[0]).ends_with(" · claude-code:7f3a")));
}

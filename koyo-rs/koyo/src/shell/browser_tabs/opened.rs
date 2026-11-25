//! Pages a page opens in a tab or a window of its own (a link to a new tab, `window.open`, a
//! sign-in's popup), which the engine made already: each gets a tab right after its opener's,
//! in front as Chrome would have shown it, of its opener's profile and maker, and goes when its
//! page closes itself, the person taken back to its opener. One an agent's page opens out of
//! the person's sight is the agent's, and stays behind unless the person set what agents open
//! to show.

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};
use koyo_browser_ipc::{AppRequest, HostEvent, PaneId};

use crate::browser::profiles::By;
use crate::browser::{BrowserView, FakeHost};
use crate::shell::Shell;
use crate::test_support::{draw, open_page_view, shell_on};

/// The numbers the host gives the pages it makes itself, far above the app's own.
const OPENED: PaneId = 1 << 40;

/// The addresses of the selected checkout's tabs, in order, and the one shown.
fn tabs(shell: &Entity<Shell>, cx: &mut TestAppContext) -> (Vec<String>, Option<String>) {
    shell.read_with(cx, |shell, _| {
        let unit = shell.workspace.branch().expect("a checkout is selected");
        let address = |tab: &crate::model::Tab| tab.first().address().unwrap_or_default().to_owned();
        let shown = shell.workspace.tab_shown().map(address);
        (unit.tabs.iter().map(address).collect(), shown)
    })
}

/// The page the tab showing host pane `pane` shows.
fn page_of(shell: &Entity<Shell>, pane: PaneId, cx: &mut TestAppContext) -> Option<Entity<BrowserView>> {
    shell.read_with(cx, |shell, cx| {
        let located = shell.workspace.locate(|shown| shown.view().is_some_and(|page| page.read(cx).host_pane() == pane))?;
        shell.workspace.pane(located.pane)?.view().cloned()
    })
}

/// Shows the tab `view`'s page is in and draws the window.
fn show(window: AnyWindowHandle, shell: &Entity<Shell>, view: &Entity<BrowserView>, cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| {
        let pane = shell.pane_showing(view).expect("the page's pane");
        assert!(shell.show_tab_holding(pane, cx));
    });
    draw(window, cx);
}

/// The host tells of a page `opener`'s page opened at `url`, made as its pane `pane`.
fn opens(host: &FakeHost, opener: &Entity<BrowserView>, (pane, url): (PaneId, &str), active: bool, cx: &mut TestAppContext) {
    let opener = opener.read_with(cx, |view, _| view.host_pane());
    host.says(HostEvent::TabOpened { pane, opener, url: url.into(), active }, cx);
}

#[gpui_kit::test]
fn a_page_a_page_opens_gets_a_tab_right_after_its_and_closing_itself_goes_back_to_it(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("page-opens-tab", cx);
    let host = FakeHost::serve("page-opens-tab-host", cx);
    let opener = open_page_view(window, &shell, "https://a.example/", cx);
    open_page_view(window, &shell, "https://b.example/", cx);
    show(window, &shell, &opener, cx);
    host.asked(cx);

    opens(&host, &opener, (OPENED + 1, "https://a.example/later"), false, cx);
    draw(window, cx);
    let (order, shown) = tabs(&shell, cx);
    assert_eq!(order, ["https://a.example/", "https://a.example/later", "https://b.example/"]);
    assert_eq!(shown.as_deref(), Some("https://a.example/"), "a tab opened behind stays behind");

    opens(&host, &opener, (OPENED + 2, "https://a.example/sign-in"), true, cx);
    draw(window, cx);
    let (order, shown) = tabs(&shell, cx);
    assert_eq!(order[1], "https://a.example/sign-in", "right after its opener's tab");
    assert_eq!(shown.as_deref(), Some("https://a.example/sign-in"), "a popup opens in front");
    let popup = page_of(&shell, OPENED + 2, cx).expect("the popup's page");
    assert_eq!(popup.read_with(cx, |view, _| view.by()), By::Person);
    let asked = host.asked(cx);
    let opened_again = |asked: &AppRequest| matches!(asked, AppRequest::Open { pane, .. } if *pane > OPENED);
    assert!(!asked.iter().any(opened_again), "the engine made them already: {asked:?}");

    host.says(HostEvent::TabClosed { pane: OPENED + 2 }, cx);
    host.says(HostEvent::Closed { pane: OPENED + 2 }, cx);
    draw(window, cx);
    let (order, shown) = tabs(&shell, cx);
    assert_eq!(order, ["https://a.example/", "https://a.example/later", "https://b.example/"], "its tab goes");
    assert_eq!(shown.as_deref(), Some("https://a.example/"), "back to the page that opened it");
}

#[gpui_kit::test]
fn a_popup_an_agents_page_opens_out_of_sight_is_the_agents_and_stays_behind(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("agent-page-opens-tab", cx);
    let host = FakeHost::serve("agent-page-opens-tab-host", cx);
    let opener = open_page_view(window, &shell, "https://a.example/", cx);
    opener.update(cx, |view, cx| {
        view.made_by(By::Agent);
        view.agent_uses(cx);
    });
    open_page_view(window, &shell, "https://b.example/", cx);

    opens(&host, &opener, (OPENED + 1, "https://a.example/sign-in"), true, cx);
    draw(window, cx);
    let (order, shown) = tabs(&shell, cx);
    assert_eq!(order[1], "https://a.example/sign-in");
    assert_eq!(shown.as_deref(), Some("https://b.example/"), "the person stays where they are");
    let popup = page_of(&shell, OPENED + 1, cx).expect("the popup's page");
    assert_eq!(popup.read_with(cx, |view, _| view.by()), By::Agent, "the agent's page's popup is the agent's");

    cx.update(|cx| crate::settings::update(cx, |settings| settings.external_control.show_what_agents_open = true));
    opens(&host, &opener, (OPENED + 2, "https://a.example/again"), true, cx);
    draw(window, cx);
    let (_, shown) = tabs(&shell, cx);
    assert_eq!(shown.as_deref(), Some("https://a.example/again"), "shown, as the person set what agents open");
}

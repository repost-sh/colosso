//! What the address bar says of a page's `DevTools` (`docs/sidebar.md` 2).

use gpui_kit::TestAppContext;

use super::{said, sidebar_shell};
use crate::browser::Dock;
use crate::test_support::{click_pane, draw, open_page};

/// The person in a page's `DevTools`, in a pane of their own beside it, sees the page's
/// address in the bar.
#[gpui_kit::test]
fn the_address_bar_says_the_pages_address_in_its_devtools(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-address-devtools"], cx);
    let opened = open_page(window, &shell, Some("https://www.example.com/docs"), cx);
    let page = shell.read_with(cx, |shell, _| {
        (shell.workspace.pane(opened)).and_then(|pane| pane.view().cloned())
    });
    let page = page.expect("its page");
    page.update(cx, |page, cx| page.inspect(None, cx));
    draw(window, cx);
    page.update(cx, |page, cx| page.dock_devtools(Dock::Detached, cx));
    draw(window, cx);
    let tools = shell.read_with(cx, |shell, _| {
        let tab = shell.workspace.tab_shown().expect("the page's tab");
        (tab.panes().into_iter()).find(|pane| pane.is_inspector()).map(|pane| pane.id)
    });
    click_pane(window, tools.expect("the DevTools' pane"), cx);
    let current = shell.read_with(cx, |shell, _| {
        (shell.workspace.tab_shown()).map(|tab| tab.current().id)
    });
    assert_eq!(current, tools, "the person is in the DevTools");

    assert_eq!(said(&shell, cx), "example.com");
}

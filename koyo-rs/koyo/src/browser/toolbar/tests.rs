//! What the strip calls a page, and a page's toolbar shown only while the address bar is in the
//! page, as the address bar's menu sets it.

use gpui_kit::TestAppContext;

use super::{display_url, tab_title};
use crate::settings::{AddressBar, SetAddressBar};
use crate::test_support::{drawn, open_page_view, shell_on};

#[test]
fn the_strip_shows_addresses_without_scheme_or_trailing_slash() {
    assert_eq!(display_url("https://www.example.com/"), "example.com");
    assert_eq!(
        display_url("http://localhost:3000/app"),
        "localhost:3000/app"
    );
    assert_eq!(display_url("about:blank"), "about:blank");
    assert_eq!(tab_title("", ""), "New tab");
    assert_eq!(tab_title("Docs", "https://docs.rs"), "Docs");
    assert_eq!(tab_title("", "https://docs.rs/"), "docs.rs");
}

/// A page has its own toolbar, with its address, only while the address bar is in the page:
/// with the address bar in the sidebar it has none. The address bar's menu
/// moves it, and the page shows the choice at once.
#[gpui_kit::test]
fn a_page_has_its_toolbar_only_while_the_address_bar_is_in_the_page(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("toolbar-follows-address-bar", cx);
    open_page_view(window, &shell, "https://example.com/", cx);
    for (address_bar, toolbar) in [(AddressBar::Page, true), (AddressBar::Sidebar, false)] {
        cx.dispatch_action(window, SetAddressBar(address_bar));
        let shown = drawn(window, "browser-address", cx).is_some();
        assert_eq!(shown, toolbar, "the address bar {address_bar:?}");
    }
    let _ = std::fs::remove_dir_all(root);
}

mod menu_bar;

use gpui_kit::TestAppContext;

use super::{check_for_updates, open_handed_pages};
use crate::model::SettingsTab;
use crate::navigation::SettingsPage;
use crate::test_support::{loose, shell_window, shown};

#[gpui_kit::test]
fn a_page_macos_hands_over_opens_unpinned_and_the_last_is_shown(cx: &mut TestAppContext) {
    cx.update(crate::test_support::init);
    let (_window, shell) = shell_window(cx);
    assert_eq!(loose(&shell, cx), Vec::<String>::new());

    cx.update(|cx| open_handed_pages(vec!["https://example.com/a".into()], cx));
    cx.run_until_parked();
    assert_eq!(loose(&shell, cx), ["https://example.com/a"]);
    assert_eq!(shown(&shell, cx), (Some("https://example.com/a".into()), true));

    cx.update(|cx| {
        let pages = ["https://example.com/b", "https://example.com/c"];
        open_handed_pages(pages.map(str::to_owned).to_vec(), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        loose(&shell, cx),
        ["https://example.com/a", "https://example.com/b", "https://example.com/c"],
        "each page handed over is an unpinned page of its own"
    );
    assert_eq!(shown(&shell, cx), (Some("https://example.com/c".into()), true));
}

#[gpui_kit::test]
fn check_for_updates_shows_about_where_the_answer_is_said(cx: &mut TestAppContext) {
    cx.update(crate::test_support::init);
    let (_window, shell) = shell_window(cx);
    let page = |cx: &mut TestAppContext| {
        shell.read_with(cx, |shell, _| shell.workspace.settings_shown().map(SettingsTab::page))
    };
    assert_eq!(page(cx), None);

    cx.update(check_for_updates);
    cx.run_until_parked();
    assert_eq!(page(cx), Some(SettingsPage::About));
}

#[gpui_kit::test]
fn a_document_macos_hands_over_opens_as_a_page_at_its_file_address(cx: &mut TestAppContext) {
    cx.update(crate::test_support::init);
    let (_window, shell) = shell_window(cx);

    let document = "file:///Users/me/Sites/a%20page.html";
    cx.update(|cx| open_handed_pages(vec![document.into()], cx));
    cx.run_until_parked();
    assert_eq!(loose(&shell, cx), [document]);
    assert_eq!(shown(&shell, cx), (Some(document.into()), true));
}

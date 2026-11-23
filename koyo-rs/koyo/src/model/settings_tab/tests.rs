//! The Settings tab on the workspace: one at most, shown over whatever tab was until it closes,
//! closed by showing anything else, and Back and Forward through the pages it showed.

use crate::model::tests::{loose, shown, with_pages};
use crate::model::{Front, SettingsTab, Workspace};
use crate::navigation::SettingsPage;

/// The page the Settings tab shows, while it's open.
fn page(workspace: &Workspace) -> Option<SettingsPage> {
    workspace.settings().map(SettingsTab::page)
}

#[test]
fn settings_opens_once_over_the_tab_shown_which_shows_again_as_it_closes() {
    let mut workspace = with_pages(&["a"]);
    assert_eq!(shown(&workspace), "a");
    assert!(workspace.open_settings(None));
    assert_eq!(page(&workspace), Some(SettingsPage::General));
    assert!(matches!(workspace.front(), Some(Front::Settings(_))));
    assert!(workspace.tab_shown().is_none(), "Settings shows over the page");
    assert!(workspace.shown_checkout_tab().is_some(), "the page is still where it was, under it");
    assert!(!workspace.open_settings(None), "open already: picked again");
    assert!(workspace.open_settings(Some(SettingsPage::Appearance)));
    assert_eq!(page(&workspace), Some(SettingsPage::Appearance), "still the one tab");
    assert!(workspace.close_settings());
    assert!(!workspace.close_settings());
    assert_eq!(shown(&workspace), "a");
}

#[test]
fn showing_any_other_tab_closes_settings() {
    let mut workspace = with_pages(&["a", "b"]);
    workspace.show_pages_of(1);
    workspace.open_settings(None);
    workspace.select_tab(0, 0, 1);
    assert_eq!((page(&workspace), shown(&workspace).as_str()), (None, "a"));

    workspace.add_loose(loose("c"), usize::MAX, false);
    workspace.open_settings(None);
    assert!(workspace.show_loose(0));
    assert_eq!((page(&workspace), shown(&workspace).as_str()), (None, "c"));

    workspace.open_settings(None);
    assert!(matches!(workspace.front(), Some(Front::Settings(_))), "covered while Settings shows");
    assert_eq!(workspace.loose_shown(), Some(0), "still the space's loose page shown");
    workspace.select_branch(0, 1);
    assert_eq!(page(&workspace), None, "another worktree");

    workspace.open_settings(None);
    workspace.show_project(0);
    assert_eq!(page(&workspace), None, "a project");
}

#[test]
fn a_pinned_page_shown_closes_settings_and_one_under_it_is_still_kept() {
    let mut workspace = with_pages(&["a", "b"]);
    let home = shown(&workspace);
    let pinned = workspace.pin_tab((0, 0, 2), home, usize::MAX).expect("pinned");
    assert!(workspace.show_pinned(pinned));
    workspace.open_settings(None);
    assert!(matches!(workspace.front(), Some(Front::Settings(_))), "covered while Settings shows");
    assert_eq!(workspace.pinned_shown(), Some(pinned), "shown again once it closes");
    assert!(workspace.show_pinned(pinned));
    assert_eq!(page(&workspace), None);
    assert!(matches!(workspace.front(), Some(Front::Pinned(at)) if at == pinned));
}

#[test]
fn the_loose_page_pinned_under_settings_is_pinned_shown_and_shows_once_settings_closes() {
    let mut workspace = with_pages(&["a"]);
    workspace.add_loose(loose("c"), usize::MAX, true);
    workspace.open_settings(None);
    let pinned = workspace.pin_loose(0, "c".to_owned(), usize::MAX).expect("pinned");
    assert!(workspace.settings().is_some(), "Settings stays over it");
    assert!(workspace.close_settings());
    assert_eq!(workspace.pinned_shown(), Some(pinned));
    assert_eq!(shown(&workspace), "c");
}

#[test]
fn back_and_forward_retrace_the_pages_the_tab_showed() {
    let mut workspace = with_pages(&[]);
    workspace.open_settings(Some(SettingsPage::General));
    let tab = workspace.settings_mut().expect("open");
    assert!(!tab.can_back());
    assert!(tab.go(SettingsPage::Appearance));
    assert!(tab.go(SettingsPage::About));
    assert!(tab.back());
    assert_eq!(tab.page(), SettingsPage::Appearance);
    assert!(tab.back());
    assert_eq!(tab.page(), SettingsPage::General);
    assert!(!tab.back(), "the first page it showed");
    assert!(tab.forward());
    assert!(tab.forward());
    assert_eq!(tab.page(), SettingsPage::About);
    assert!(!tab.can_forward());
}

#[test]
fn going_to_a_page_forgets_the_forward_pages_and_the_page_shown_records_nothing() {
    let mut workspace = with_pages(&[]);
    workspace.open_settings(Some(SettingsPage::About));
    let tab = workspace.settings_mut().expect("open");
    assert!(!tab.go(SettingsPage::About));
    assert!(!tab.can_back());
    assert!(tab.go(SettingsPage::Chat));
    tab.back();
    assert!(tab.can_forward());
    tab.go(SettingsPage::General);
    assert!(!tab.can_forward());
}

#[test]
fn settings_opens_again_on_the_page_it_showed_last() {
    let mut workspace = with_pages(&[]);
    workspace.open_settings(Some(SettingsPage::KeyboardShortcuts));
    workspace.close_settings();
    workspace.open_settings(None);
    assert_eq!(page(&workspace), Some(SettingsPage::KeyboardShortcuts));
    let tab = workspace.settings().expect("open");
    assert!(!tab.can_back(), "a new tab, with no pages behind it");
}

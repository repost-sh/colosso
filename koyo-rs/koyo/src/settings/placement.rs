//! Where the tabs, the address bar and the selected branch's Changes and Files buttons go
//! (`docs/layout.md`, Settings), the menus that pick each, and the actions they pick by. The
//! buttons go on the top rail only while the tabs are on it: with the tabs in the sidebar there
//! is no top rail, so they sit on the branch's row, and where the person put them is kept for
//! the tabs' return.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App};
use serde::{Deserialize, Serialize};

use super::{LoosePages, Settings, checked_menu, update};

/// Where every tab is, the pinned pages, the unpinned ones, the Settings tab and the selected
/// branch's tabs together: across the top rail, or down the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Tabs {
    #[default]
    TopRail,
    Sidebar,
}

/// Moves the tabs, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetTabs(pub Tabs);

impl Tabs {
    pub const ALL: [Tabs; 2] = [Tabs::TopRail, Tabs::Sidebar];

    pub fn label(self) -> &'static str {
        match self {
            Tabs::TopRail => "Top rail",
            Tabs::Sidebar => "Sidebar",
        }
    }

    /// A menu of where the tabs can go, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(Tabs::ALL, self, Tabs::label, |tabs| Box::new(SetTabs(tabs)))
    }
}

/// Where the address of the page shown is, with back, forward and reload: in each page's own
/// toolbar, or in a field at the top of the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AddressBar {
    Page,
    #[default]
    Sidebar,
}

/// Moves the address bar, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetAddressBar(pub AddressBar);

impl AddressBar {
    pub const ALL: [AddressBar; 2] = [AddressBar::Page, AddressBar::Sidebar];

    pub fn label(self) -> &'static str {
        match self {
            AddressBar::Page => "In the page",
            AddressBar::Sidebar => "In the sidebar",
        }
    }

    /// A menu of where the address bar can go, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(AddressBar::ALL, self, AddressBar::label, |bar| {
            Box::new(SetAddressBar(bar))
        })
    }
}

/// Where the selected branch's Changes and Files buttons are: on the branch's row in the
/// sidebar, or at the top rail's right end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ChangesAndFiles {
    Worktree,
    #[default]
    TopRail,
}

/// Moves the Changes and Files buttons, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetChangesAndFiles(pub ChangesAndFiles);

impl ChangesAndFiles {
    pub const ALL: [ChangesAndFiles; 2] = [ChangesAndFiles::Worktree, ChangesAndFiles::TopRail];

    pub fn label(self) -> &'static str {
        match self {
            ChangesAndFiles::Worktree => "Worktree",
            ChangesAndFiles::TopRail => "Top rail",
        }
    }

    /// Whether the buttons can go here with the tabs where `tabs` has them: the branch's row
    /// always, the top rail only while the tabs are on it.
    fn offered(self, tabs: Tabs) -> bool {
        match self {
            ChangesAndFiles::Worktree => true,
            ChangesAndFiles::TopRail => tabs == Tabs::TopRail,
        }
    }

    /// Where the buttons put here are with the tabs where `tabs` has them: here, or on the
    /// branch's row while the tabs in the sidebar leave no top rail.
    fn shown(self, tabs: Tabs) -> ChangesAndFiles {
        if self.offered(tabs) {
            self
        } else {
            ChangesAndFiles::Worktree
        }
    }

    /// A menu of where the buttons can go with the tabs where `tabs` has them: where the
    /// buttons put at `self` are checked, and the top rail greyed while the tabs are in the
    /// sidebar.
    fn menu(self, tabs: Tabs) -> NativeMenu {
        let shown = self.shown(tabs);
        ChangesAndFiles::ALL
            .into_iter()
            .fold(NativeMenu::new(), |menu, place| {
                let pick = Box::new(SetChangesAndFiles(place));
                if place.offered(tabs) {
                    menu.menu_with_check(place.label(), place == shown, pick)
                } else {
                    menu.menu_with_disabled(place.label(), true, pick)
                }
            })
    }
}

impl Settings {
    /// Whether the tabs are down the sidebar rather than across the top rail.
    pub fn tabs_in_sidebar(&self) -> bool {
        self.tabs == Tabs::Sidebar
    }

    /// Whether every space lists the same unpinned pages, rather than each its own.
    pub fn loose_pages_shared(&self) -> bool {
        self.loose_pages == LoosePages::Shared
    }

    /// Whether the address bar is in the sidebar, and back, forward and reload with it in the
    /// window's top row; otherwise each page's toolbar has them. They're never in both
    /// (`docs/layout.md`).
    pub fn address_in_sidebar(&self) -> bool {
        self.address_bar == AddressBar::Sidebar
    }

    /// Where the Changes and Files buttons are: where the person put them, or on the branch's
    /// row while the tabs in the sidebar leave no top rail.
    pub fn changes_and_files_shown(&self) -> ChangesAndFiles {
        self.changes_and_files.shown(self.tabs)
    }

    /// Whether the selected branch's Changes and Files buttons are at the top rail's end, where
    /// they're shown (`changes_and_files_shown`); otherwise they're on the branch's row in the
    /// sidebar (`Shell::row_panel_buttons`).
    pub fn panel_buttons_on_rail(&self) -> bool {
        self.changes_and_files_shown() == ChangesAndFiles::TopRail
    }

    /// The menu of where the Changes and Files buttons go: where they're shown checked, and the
    /// top rail greyed while the tabs are in the sidebar.
    pub fn changes_and_files_menu(&self) -> NativeMenu {
        self.changes_and_files.menu(self.tabs)
    }
}

/// Sets each as its menu picks it.
pub(super) fn init(cx: &mut App) {
    cx.on_action(|SetTabs(tabs): &SetTabs, cx| {
        update(cx, |settings| settings.tabs = *tabs);
    });
    cx.on_action(|SetAddressBar(bar): &SetAddressBar, cx| {
        update(cx, |settings| settings.address_bar = *bar);
    });
    cx.on_action(|SetChangesAndFiles(place): &SetChangesAndFiles, cx| {
        update(cx, |settings| settings.changes_and_files = *place);
    });
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use serde_json::json;

    use super::*;
    use crate::settings;
    use crate::test_support::init_settings;

    /// A new file is written with the layout's defaults as `docs/layout.md` has them, a file
    /// from before them reads as the defaults, and a stored choice wins.
    #[test]
    fn a_new_file_has_the_layouts_defaults_and_a_stored_choice_is_kept() {
        let written = serde_json::to_value(Settings::default()).unwrap();
        let spec = json!({
            "tabs": "TopRail",
            "address_bar": "Sidebar",
            "changes_and_files": "TopRail",
            "loose_pages": "Shared",
            "pinned_zone_dismissed": false,
        });
        for (key, value) in spec.as_object().unwrap() {
            assert_eq!(&written[key], value, "{key}");
        }
        let before: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            (before.tabs, before.address_bar, before.changes_and_files),
            (Tabs::TopRail, AddressBar::Sidebar, ChangesAndFiles::TopRail),
        );
        let stored = json!({
            "tabs": "Sidebar",
            "address_bar": "Page",
            "changes_and_files": "Worktree",
        });
        let stored: Settings = serde_json::from_value(stored).unwrap();
        assert_eq!(
            (stored.tabs, stored.address_bar, stored.changes_and_files),
            (Tabs::Sidebar, AddressBar::Page, ChangesAndFiles::Worktree),
        );
    }

    /// What each menu picks is the setting from then on.
    #[gpui_kit::test]
    fn each_menus_pick_is_the_setting(cx: &mut TestAppContext) {
        let picked = |cx: &mut TestAppContext| {
            cx.read(|cx| {
                let settings = settings::get(cx);
                (settings.tabs, settings.address_bar, settings.changes_and_files)
            })
        };
        cx.update(|cx| {
            init_settings(cx);
            cx.dispatch_action(&SetTabs(Tabs::Sidebar));
            cx.dispatch_action(&SetAddressBar(AddressBar::Page));
            cx.dispatch_action(&SetChangesAndFiles(ChangesAndFiles::Worktree));
        });
        assert_eq!(
            picked(cx),
            (Tabs::Sidebar, AddressBar::Page, ChangesAndFiles::Worktree),
        );
        cx.update(|cx| {
            cx.dispatch_action(&SetTabs(Tabs::TopRail));
            cx.dispatch_action(&SetAddressBar(AddressBar::Sidebar));
            cx.dispatch_action(&SetChangesAndFiles(ChangesAndFiles::TopRail));
        });
        assert_eq!(
            picked(cx),
            (Tabs::TopRail, AddressBar::Sidebar, ChangesAndFiles::TopRail),
        );
    }

    /// With the tabs in the sidebar the Changes and Files buttons are on the branch's row and
    /// the top rail is greyed in their menu, while where the person put them is kept: the tabs
    /// back on the top rail take them back there.
    #[test]
    fn changes_and_files_leave_the_top_rail_with_the_tabs_and_come_back_with_them() {
        let offered = |tabs| ChangesAndFiles::ALL.map(|place| place.offered(tabs));
        assert_eq!(offered(Tabs::TopRail), [true, true]);
        assert_eq!(offered(Tabs::Sidebar), [true, false], "the top rail greyed");
        let mut settings = Settings::default();
        assert_eq!(settings.changes_and_files_shown(), ChangesAndFiles::TopRail);
        settings.tabs = Tabs::Sidebar;
        assert_eq!(settings.changes_and_files_shown(), ChangesAndFiles::Worktree);
        assert_eq!(settings.changes_and_files, ChangesAndFiles::TopRail, "kept");
        settings.tabs = Tabs::TopRail;
        assert_eq!(settings.changes_and_files_shown(), ChangesAndFiles::TopRail);
        settings.changes_and_files = ChangesAndFiles::Worktree;
        for tabs in Tabs::ALL {
            settings.tabs = tabs;
            assert_eq!(settings.changes_and_files_shown(), ChangesAndFiles::Worktree, "{tabs:?}");
        }
    }
}

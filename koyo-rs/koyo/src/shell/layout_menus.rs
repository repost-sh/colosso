//! How a right-click opens a menu (`with_menu`), and the menus a right-click on the window's
//! empty places opens (`docs/layout.md`, Right-click menus): the sidebar's, the top rail's, the
//! tab area's, the address bar's and the Changes and Files buttons'. Each picks the layout's
//! settings from where the person is, a choice applying at once as the Settings row does, and
//! ends in Settings… (`with_settings`). The theme picker goes on the press that opens any of
//! them, as every floating window does on a press in the window (`popup::listener`), so none
//! closes it itself.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{App, InteractiveElement, MouseButton, MouseDownEvent, Window};

use super::spaces::space_menu;
use super::{EditTheme, ToggleSidebar};
use crate::navigation::OpenSettings;
use crate::settings::{self, LoosePages, SetLoosePages, SetTabs, Tabs, checked_items, checked_menu};
use crate::theme;

/// Has `element` open the menu `build` makes where it is right-clicked, rather than any menu of
/// what it's in.
pub fn with_menu<E: InteractiveElement>(
    element: E,
    build: impl Fn(&mut Window, &mut App) -> NativeMenu + 'static,
) -> E {
    element.on_mouse_down(
        MouseButton::Right,
        move |event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            build(window, cx).show(event.position, window, cx);
        },
    )
}

/// `menu` with Settings… at its end, after a separator.
pub fn with_settings(menu: NativeMenu) -> NativeMenu {
    menu.separator().menu("Settings…", Box::new(OpenSettings))
}

/// The menu of the sidebar's empty space, its top row's included: the space shown's own items,
/// then the top rail's. A row with a menu of its own opens that instead.
pub fn sidebar_menu(cx: &App) -> NativeMenu {
    let space = settings::get(cx).shown_space_id();
    layout_items(space_menu(space, cx).separator(), cx)
}

/// `area`, a part of the tab area, on the rail or in the sidebar, opens the tab area's menu as
/// its empty space is right-clicked; what's in it with a menu of its own opens that instead.
pub fn tab_area<E: InteractiveElement>(area: E) -> E {
    with_menu(area, |_, cx| tab_area_menu(cx))
}

/// The menu of the tab area's empty space, on the rail or in the sidebar: where the tabs sit and
/// whose the unpinned pages are, the current choice of each checked. A tab with a menu of its
/// own opens that instead.
pub fn tab_area_menu(cx: &App) -> NativeMenu {
    let settings = settings::get(cx);
    let tabs = checked_menu(
        Tabs::ALL,
        settings.tabs,
        |tabs: Tabs| format!("Tabs on the {}", tabs.label()),
        |tabs| Box::new(SetTabs(tabs)),
    );
    let pages = checked_items(
        tabs,
        [LoosePages::Shared, LoosePages::PerSpace],
        settings.loose_pages,
        |pages: LoosePages| format!("Unpinned pages: {}", pages.label()),
        |pages| Box::new(SetLoosePages(pages)),
    );
    with_settings(pages)
}

/// The menu either address bar opens, the sidebar's or a page's, rather than any menu of what
/// the bar is in: where the address bar is, checked, then Settings… (`docs/layout.md`).
pub fn address_bar_menu(cx: &App) -> NativeMenu {
    with_settings(settings::get(cx).address_bar.menu())
}

/// The menu a Changes or Files button opens as it's right-clicked (`docs/layout.md`): where
/// the two are placed, the place they're shown at checked and the top rail greyed while the
/// tabs are in the sidebar, then Settings….
pub fn placement_menu(cx: &App) -> NativeMenu {
    with_settings(settings::get(cx).changes_and_files_menu())
}

/// `menu` with the items the sidebar's and the top rail's menus share appended: the theme, the
/// sidebar, each layout setting's choices with the current one checked, and Settings. Appended
/// to an empty menu, it's the menu of the top rail's empty glass, and of what else in the window
/// has no menu of its own.
pub(super) fn layout_items(menu: NativeMenu, cx: &App) -> NativeMenu {
    let settings = settings::get(cx);
    let menu = menu
        .menu("Edit Theme…", Box::new(EditTheme))
        .submenu("Appearance", theme::preference(cx).menu())
        .menu("Toggle Sidebar", Box::new(ToggleSidebar))
        .submenu("Tabs", settings.tabs.menu())
        .submenu("Address bar", settings.address_bar.menu())
        .submenu("Changes and Files", settings.changes_and_files_menu())
        .submenu("Unpinned pages", settings.loose_pages.menu());
    with_settings(menu)
}

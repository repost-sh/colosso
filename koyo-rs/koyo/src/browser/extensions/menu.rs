//! What a page does with extensions (`docs/extensions.md` 5 and 6): a press on an icon runs
//! the extension's action, Chrome's own; a right press opens the icon's menu, the app's
//! native one, whose picks come back as actions on the window, as a page's menu's do.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, Bounds, Context, Pixels, Point, Window};
use koyo_browser_ipc::{AppRequest, Corner, ExtensionChange, PaneId, ScreenRect};

use super::{MANAGE, STORE, Shown};
use crate::browser::view::{BrowserEvent, BrowserView};

/// What a row of an icon's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconPick {
    /// Its home page, in a tab: the Web Store's page for one from there.
    Home,
    /// Its options page, in a tab.
    Options,
    /// Pins it to the toolbar, or takes it off.
    Pin(bool),
    /// Removes it from the profile.
    Remove,
    /// Chrome's own page about it, in a tab.
    Manage,
}

/// A row picked from the menu of `extension`'s icon, for the page `pane`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct ExtensionPick {
    pub pane: PaneId,
    pub extension: String,
    pub pick: IconPick,
}

/// Which of its top corners a popup hangs by from under `icon`, in a window `width` wide:
/// the one that keeps it over the window. From an icon in the window's left half (the site
/// controls of a sidebar's address field) it opens to the right, by its top left; from one
/// in the right half (a toolbar's end) to the left, as under Chrome's own toolbar.
fn hangs_by(icon: Bounds<Pixels>, width: Pixels) -> Corner {
    if icon.center().x < width / 2. { Corner::Left } else { Corner::Right }
}

/// Where `icon`, drawn in `window`, is on the screen, in points from the main screen's bottom
/// left, as `AppKit` counts them; none for a test's window.
fn on_screen(icon: Bounds<Pixels>, window: &Window) -> Option<ScreenRect> {
    let [left, top] = crate::native::window::on_screen(window, icon.origin)?;
    let [right, bottom] = crate::native::window::on_screen(window, icon.bottom_right())?;
    Some(ScreenRect { x: left.min(right), y: bottom.min(top), width: (right - left).abs(), height: (top - bottom).abs() })
}

/// The rows of `shown`'s menu, by what each says: a separator is none.
fn rows(shown: &Shown) -> Vec<Option<(String, IconPick, bool)>> {
    let mut rows = vec![Some((shown.name.clone(), IconPick::Home, !shown.homepage.is_empty())), None];
    if !shown.options.is_empty() {
        rows.push(Some(("Options".into(), IconPick::Options, true)));
    }
    let pin = if shown.pinned { "Unpin from Toolbar" } else { "Pin to Toolbar" };
    rows.push(Some((pin.into(), IconPick::Pin(!shown.pinned), true)));
    rows.push(Some(("Manage Extension".into(), IconPick::Manage, true)));
    rows
}

/// The menu of `shown`'s icon for the page `pane`. Removing is asked once more: it's a
/// submenu's only row, which names what goes.
fn menu(shown: &Shown, pane: PaneId) -> NativeMenu {
    let pick = |pick| Box::new(ExtensionPick { pane, extension: shown.id.clone(), pick });
    let mut menu = NativeMenu::new();
    for row in rows(shown) {
        menu = match row {
            Some((label, does, enabled)) => menu.menu_with_disabled(label, !enabled, pick(does)),
            None => menu.separator(),
        };
    }
    if shown.removable {
        let sure = NativeMenu::new().menu(format!("Remove “{}”", shown.name), pick(IconPick::Remove));
        menu = menu.separator().submenu("Remove from Koyo", sure);
    }
    menu
}

impl BrowserView {
    /// The icons of the page's profile's extensions, as this page shows them; with
    /// `pinned_only`, those pinned to the toolbar.
    pub(in crate::browser) fn extensions(&self, pinned_only: bool, cx: &gpui_kit::App) -> Vec<Shown> {
        let Some(profile) = self.profile().filter(|_| self.is_open(cx)) else {
            return Vec::new();
        };
        self.browsers.read(cx).extensions.shown(profile, self.pane, pinned_only)
    }

    /// How many icons the page's profile's extensions have; none where the engine doesn't
    /// run them with icons at all (`Store::known`).
    pub(in crate::browser) fn extension_count(&self, cx: &gpui_kit::App) -> Option<usize> {
        let profile = self.profile()?;
        let store = &self.browsers.read(cx).extensions;
        (self.is_open(cx) && store.known(profile)).then(|| store.count(profile))
    }

    /// How many times what the engine said of extensions changed (`Store::changes`).
    pub(in crate::browser) fn extension_changes(&self, cx: &gpui_kit::App) -> u64 {
        self.browsers.read(cx).extensions.changes()
    }

    /// The pointer went down on `extension`'s icon: its popup open over the page closes, and
    /// the press it starts opens none, as in Chrome.
    pub(in crate::browser) fn extension_pointer_down(&mut self, extension: &str, cx: &mut Context<Self>) {
        let (pane, extension) = (self.pane, extension.to_owned());
        self.send_to_page(AppRequest::ExtensionPointerDown { pane, extension }, cx);
    }

    /// A press on `extension`'s icon, drawn at `icon` in `window`: Chrome runs its action on
    /// the page, and a popup it opens hangs from under the icon, towards the window's middle,
    /// so it's over the window from an icon at either side of it.
    pub(in crate::browser) fn extension_act(
        &mut self,
        extension: &str,
        icon: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let corner = hangs_by(icon, window.viewport_size().width);
        let Some(bounds) = on_screen(icon, window) else {
            return;
        };
        let (pane, extension) = (self.pane, extension.to_owned());
        self.send_to_page(AppRequest::ExtensionAct { pane, extension, bounds, corner }, cx);
    }

    /// A right press on `shown`'s icon: its menu opens at `at` in `window`.
    pub(in crate::browser) fn extension_menu(
        &mut self,
        shown: &Shown,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if cfg!(feature = "visual-check") {
            return;
        }
        menu(shown, self.pane).show(at, window, cx);
    }

    /// The person picked `pick` from the menu of `extension`'s icon.
    pub(in crate::browser) fn extension_picked(
        &mut self,
        extension: &str,
        pick: IconPick,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = self.profile().map(str::to_owned) else {
            return;
        };
        let known = self.browsers.read(cx).extensions.get(&profile, extension).cloned();
        let change = |change| AppRequest::ChangeExtension {
            profile: profile.clone(),
            extension: extension.to_owned(),
            change,
        };
        match pick {
            IconPick::Home => {
                if let Some(known) = known.filter(|known| !known.homepage_url.is_empty()) {
                    cx.emit(BrowserEvent::GoToTab(known.homepage_url));
                }
            }
            IconPick::Options => {
                if let Some(known) = known.filter(|known| !known.options_url.is_empty()) {
                    cx.emit(BrowserEvent::GoToTab(known.options_url));
                }
            }
            IconPick::Pin(on) => self.send_to_page(change(ExtensionChange::Pinned { on }), cx),
            IconPick::Remove => self.send_to_page(change(ExtensionChange::Remove), cx),
            IconPick::Manage => cx.emit(BrowserEvent::GoToTab(format!("{MANAGE}?id={extension}"))),
        }
    }

    /// The Chrome Web Store's extensions, in a tab beside this one.
    pub(in crate::browser) fn open_extension_store(&mut self, cx: &mut Context<Self>) {
        cx.emit(BrowserEvent::GoToTab(STORE.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{point, px, size};

    #[test]
    fn a_popup_opens_towards_the_windows_middle() {
        let icon = |x: f32| Bounds::new(point(px(x), px(40.)), size(px(26.), px(26.)));
        assert_eq!(hangs_by(icon(226.), px(1920.)), Corner::Left);
        assert_eq!(hangs_by(icon(1860.), px(1920.)), Corner::Right);
    }

    fn shown(pinned: bool, options: &str, homepage: &str) -> Shown {
        Shown {
            id: "abc".into(),
            name: "Block".into(),
            title: "Block".into(),
            icon: None,
            badge: None,
            enabled: true,
            pinned,
            removable: true,
            options: options.into(),
            homepage: homepage.into(),
        }
    }

    fn labels(shown: &Shown) -> Vec<String> {
        rows(shown).into_iter().map(|row| row.map_or_else(|| "-".into(), |(label, ..)| label)).collect()
    }

    #[test]
    fn an_icons_menu_names_the_extension_and_what_can_be_done_with_it() {
        let with_all = shown(false, "chrome-extension://abc/options.html", "https://example.com/");
        assert_eq!(labels(&with_all), ["Block", "-", "Options", "Pin to Toolbar", "Manage Extension"]);
        let bare = shown(true, "", "");
        assert_eq!(labels(&bare), ["Block", "-", "Unpin from Toolbar", "Manage Extension"]);
        assert_eq!(rows(&bare)[0], Some(("Block".into(), IconPick::Home, false)));
        assert_eq!(rows(&bare)[2], Some(("Unpin from Toolbar".into(), IconPick::Pin(false), true)));
    }
}

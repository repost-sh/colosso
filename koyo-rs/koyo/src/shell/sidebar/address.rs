//! The top of the sidebar while the address bar is in it (`docs/layout.md`,
//! `docs/sidebar.md` 1, 2): back, forward and reload for the page shown, in the window's
//! top row, and the address bar, which says what the pane the person is in shows: a page's
//! address, a terminal's program, a chat's name, "File" or "Settings". Clicked with a page
//! shown, it opens the launcher on the page, as ⌘L does; with no tab open it asks for an
//! address, and a click opens a new page with its address ready to type in; right-clicked, the
//! menu of where the address bar is. With the address bar in the page, each page's toolbar has them instead
//! (`Settings::address_in_sidebar`).
//!
//! Both are views of their own, which the shell tells what's shown (`Shell::tell_sidebar_top`).

use gpui_kit::Focusable as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    px,
};

use super::Look;
use crate::shell::shown_page::{PageBack, PageForward, PageReload};
use crate::browser::{BrowserView, Controls};
use crate::launcher::OpenAddress;
use crate::model::{Content, Front, Pane, SettingsTab};
use crate::settings;
use crate::shell::{Shell, action_button, address_bar_menu, with_menu};

/// What the bar says while it has no address to say: with no tab open, where a click opens a
/// page to type one in, and on a new page's start page.
const ASKS_FOR_ADDRESS: &str = "Search or enter address";

/// The views at the top of the sidebar.
pub struct Top {
    controls: Entity<PageControls>,
    bar: Entity<AddressBar>,
}

/// What the address bar is told is shown.
#[derive(Clone, Default, PartialEq)]
struct Shown {
    /// What the bar says, and whether that's a placeholder, said dimmer.
    said: (SharedString, bool),
    /// The page the bar's click and buttons act on: the shell's `shown_page`.
    page: Option<Entity<BrowserView>>,
    /// Whether a click on the bar gives an address to type: a page is shown, whose address
    /// it changes, or no tab is open, and a new page opens for one (`Shell::open_address`).
    takes_address: bool,
    /// The pages of the tab shown, whose address field the bar is while one has the keyboard.
    pages: Vec<Entity<BrowserView>>,
}

impl Top {
    pub fn new(cx: &mut App) -> Top {
        Top {
            controls: cx.new(|_| PageControls::default()),
            bar: cx.new(|_| AddressBar::default()),
        }
    }

    /// Back, forward and reload, for the window's top row: the top rail's, or the sidebar's
    /// top row with no rail. Shown only while the address bar is in the sidebar
    /// (`Settings::address_in_sidebar`).
    pub fn controls(&self) -> Entity<PageControls> {
        self.controls.clone()
    }

    /// The address bar, for the sidebar's head while the address bar is in the sidebar
    /// (`Settings::address_in_sidebar`).
    pub fn bar(&self) -> Entity<AddressBar> {
        self.bar.clone()
    }

    /// `shown` is what's shown: each view draws again only when what it shows of it changed.
    fn tell(&self, shown: Shown, cx: &mut App) {
        PageControls::set_page(&self.controls, shown.page.clone(), cx);
        AddressBar::set_shown(&self.bar, shown, cx);
    }
}

impl Shell {
    /// Whether no tab is open where the person is: no tab is shown, and Settings isn't either.
    pub(crate) fn no_tab_open(&self) -> bool {
        let settings = matches!(self.workspace.front(), Some(Front::Settings(_)));
        !settings && self.workspace.tab_shown().is_none()
    }

    /// Tells the top of the sidebar what's shown while the address bar is in the sidebar; with
    /// it in the page, that nothing is, so it holds no page the page's own toolbar shows.
    pub(crate) fn tell_sidebar_top(&self, cx: &mut App) {
        let shown = if settings::get(cx).address_in_sidebar() {
            let page = self.shown_page();
            Shown {
                said: said(self, page.as_ref()),
                takes_address: page.is_some() || self.no_tab_open(),
                page,
                pages: self.pages_shown(),
            }
        } else {
            Shown::default()
        };
        self.sidebar_top.tell(shown, cx);
    }
}

/// Back, forward and reload, for the page shown; greyed out with no page shown.
#[derive(Default)]
pub struct PageControls {
    page: Option<Entity<BrowserView>>,
    /// What the buttons can do for the page, as it last said.
    controls: Controls,
    _page_changed: Option<Subscription>,
}

impl PageControls {
    /// The buttons of `this` act on `page` from now on; they're updated only when it's another.
    fn set_page(this: &Entity<Self>, page: Option<Entity<BrowserView>>, cx: &mut App) {
        if this.read(cx).page == page {
            return;
        }
        this.update(cx, |this, cx| {
            this._page_changed = (page.as_ref()).map(|page| {
                cx.observe(page, |this, page, cx| {
                    let controls = page.read(cx).controls(cx);
                    this.show(controls, cx);
                })
            });
            let controls = (page.as_ref()).map(|page| page.read(cx).controls(cx));
            this.page = page;
            this.show(controls.unwrap_or_default(), cx);
        });
    }

    fn show(&mut self, controls: Controls, cx: &mut Context<Self>) {
        if controls != self.controls {
            self.controls = controls;
            cx.notify();
        }
    }
}

impl Render for PageControls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let controls = self.controls;
        let (reload, said) = if controls.loading {
            (IconName::X, "Stop")
        } else {
            (IconName::RotateCw, "Reload")
        };
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                action_button("page-back", IconName::ArrowLeft, "Back", PageBack, cx)
                    .disabled(!controls.back),
            )
            .child(
                action_button(
                    "page-forward",
                    IconName::ArrowRight,
                    "Forward",
                    PageForward,
                    cx,
                )
                .disabled(!controls.forward),
            )
            .child(
                action_button("page-reload", reload, said, PageReload, cx)
                    .disabled(!controls.reload),
            )
    }
}

/// What the address bar says while `shown` is the page shown (`Shell::shown_page`), and
/// whether that's a placeholder, said dimmer. A page's address is the one kept in the pane
/// showing it, so a page's `DevTools` say their page's.
pub(super) fn said(shell: &Shell, shown: Option<&Entity<BrowserView>>) -> (SharedString, bool) {
    if let Some(Front::Settings(_)) = shell.workspace.front() {
        return (SettingsTab::TITLE.into(), false);
    }
    let Some(tab) = shell.workspace.tab_shown() else {
        return (ASKS_FOR_ADDRESS.into(), true);
    };
    let pane = tab.current();
    match &pane.content {
        Content::Browser(browser) => {
            let showing = shown.and_then(|page| shell.workspace.pane(shell.pane_showing(page)?));
            address_said(showing.map_or(browser.address(), Pane::address).unwrap_or_default())
        }
        Content::Terminal(_) | Content::Chat(_) | Content::Desktop(_) | Content::Device(_) => {
            (pane.title.clone(), false)
        }
        Content::Editor(_) => ("File".into(), false),
    }
}

/// What the address bar says of a page at `address`.
fn address_said(address: &str) -> (SharedString, bool) {
    if address.is_empty() {
        (ASKS_FOR_ADDRESS.into(), true)
    } else {
        (host_of(address).into(), false)
    }
}

/// A web address as the Arc browser shows it: its host, less `www.`; anything else as it is.
pub(super) fn host_of(address: &str) -> String {
    crate::history::host(address).unwrap_or_else(|| address.to_owned())
}

/// The address bar. It says what's shown, and with a page shown a click on it is ⌘L: the
/// launcher opens on the page, its address in the field to change (`Shell::open_address`). With
/// no tab open a click is ⌘L too, and opens a new page with its address ready to type in. With
/// a tab of another kind shown there's no address, and a click does nothing. While a page's own address field has
/// the keyboard, as a new tab's start page gives it, the bar is that field, the page having no
/// toolbar while the address bar is in the sidebar. Right-clicked while it isn't that field, it
/// opens the menu of where the address bar is.
#[derive(Default)]
pub struct AddressBar {
    shown: Shown,
}

impl AddressBar {
    /// `bar` shows `shown` from now on; it's updated only when that's new.
    fn set_shown(bar: &Entity<Self>, shown: Shown, cx: &mut App) {
        if bar.read(cx).shown != shown {
            bar.update(cx, |bar, cx| {
                bar.shown = shown;
                cx.notify();
            });
        }
    }
}

impl Render for AddressBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Shown {
            said,
            page,
            takes_address,
            pages,
        } = &self.shown;
        let look = Look::of(cx);
        let p = look.p;
        let (text, placeholder) = said.clone();
        let editing = (pages.iter())
            .map(|page| page.read(cx).address_field().clone())
            .find(|field| field.read(cx).focus_handle(cx).is_focused(window));
        let (hovered, hover) = crate::theme::hover::eased(
            "sidebar-address",
            crate::browser::SITE_BUTTONS_FADE,
            window,
            cx,
        );
        let ends = page.as_ref().filter(|_| editing.is_none()).and_then(|page| {
            let pinned = crate::browser::pinned_extensions(page, cx);
            crate::browser::site_buttons(page.read(cx), page, p.text.opacity(0.7), hovered, pinned, cx)
        });
        div()
            .id("sidebar-address")
            .on_hover(hover)
            .debug_selector(|| "sidebar-address".to_owned())
            .h(px(34.))
            .flex_shrink_0()
            .pl(px(12.))
            .pr(px(6.))
            .gap(px(4.))
            .flex()
            .items_center()
            .rounded(px(10.))
            .bg(look.field)
            .text_sm()
            .text_color(if placeholder {
                p.muted
            } else {
                p.text.opacity(0.85)
            })
            .map(|bar| {
                if let Some(field) = editing {
                    bar.cursor_text().key_context("BrowserAddress").child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(Input::new(&field).appearance(false).small()),
                    )
                } else {
                    let bar = (bar.child(div().min_w_0().flex_1().truncate().child(text)))
                        .when(*takes_address, |bar| {
                            bar.cursor_text().on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(OpenAddress), cx);
                            })
                        });
                    with_menu(bar, |_, cx| address_bar_menu(cx)).children(ends)
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::host_of;
    use crate::settings::{self, AddressBar};
    use crate::test_support::{draw, open_page_view, shell_on};

    /// The sidebar's address bar is told the page shown while the address bar is in the
    /// sidebar, and holds no page while it's in the page, whose own toolbar has it.
    #[gpui_kit::test]
    fn the_sidebars_address_bar_follows_the_page_shown_only_while_it_is_in_the_sidebar(
        cx: &mut TestAppContext,
    ) {
        let (window, shell, root) = shell_on("sidebar-top-follows-address-bar", cx);
        open_page_view(window, &shell, "https://www.example.com/docs", cx);
        let told = |cx: &TestAppContext| {
            cx.read(|cx| {
                let bar = shell.read(cx).sidebar_top.bar();
                let shown = &bar.read(cx).shown;
                (shown.said.0.to_string(), shown.page.is_some())
            })
        };
        let places = [(AddressBar::Sidebar, "example.com"), (AddressBar::Page, "")];
        for (address_bar, said) in places {
            cx.update(|cx| settings::update(cx, |settings| settings.address_bar = address_bar));
            draw(window, cx);
            let page = address_bar == AddressBar::Sidebar;
            let what = format!("the address bar {address_bar:?}");
            assert_eq!(told(cx), (said.to_owned(), page), "{what}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_address_shows_as_its_host() {
        assert_eq!(host_of("https://www.example.com/path?q=1"), "example.com");
        assert_eq!(host_of("http://me@localhost:3000/"), "localhost:3000");
        assert_eq!(host_of("about:blank"), "about:blank");
        assert_eq!(
            host_of("file:///Users/me/page.html"),
            "file:///Users/me/page.html"
        );
    }
}

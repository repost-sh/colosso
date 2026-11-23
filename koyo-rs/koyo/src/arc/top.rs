//! The top of the Arc sidebar (`docs/arc-sidebar.md` 1, 2): back, forward and reload for the
//! page shown, at the sidebar's edge of the top rail, and under them the address bar, which says
//! what the pane the person is in shows: a page's address, a terminal's program, a chat's name,
//! "File" or "Settings". Clicked with a page shown, it opens the launcher on the page, as ⌘L
//! does.
//!
//! Both are views of their own, which the shell tells what's shown (`Shell::tell_arc_top`).

use gpui_kit::Focusable as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px,
};

use super::Look;
use super::actions::{PageBack, PageForward, PageReload};
use crate::browser::{BrowserView, Controls};
use crate::launcher::OpenAddress;
use crate::model::{Content, Front, Pane, SettingsTab};
use crate::settings::{self, Layout};
use crate::shell::{Shell, action_button};

/// The views at the top of the Arc sidebar.
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

    /// Back, forward and reload, for the top rail.
    pub fn controls(&self) -> Entity<PageControls> {
        self.controls.clone()
    }

    /// The address bar, for the sidebar's head.
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
    /// Tells the top of the Arc sidebar what's shown, in that layout.
    pub(crate) fn tell_arc_top(&self, cx: &mut App) {
        if settings::get(cx).layout != Layout::Arc {
            return;
        }
        let page = self.shown_page();
        let shown = Shown {
            said: said(self, page.as_ref()),
            page,
            pages: self.pages_shown(),
        };
        self.arc_top.tell(shown, cx);
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
                action_button("arc-back", IconName::ArrowLeft, "Back", PageBack, cx)
                    .disabled(!controls.back),
            )
            .child(
                action_button(
                    "arc-forward",
                    IconName::ArrowRight,
                    "Forward",
                    PageForward,
                    cx,
                )
                .disabled(!controls.forward),
            )
            .child(
                action_button("arc-reload", reload, said, PageReload, cx)
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
        return ("No tab open".into(), true);
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
        ("Search or enter address".into(), true)
    } else {
        (host_of(address).into(), false)
    }
}

/// A web address as Arc shows it: its host, less `www.`; anything else as it is.
pub(super) fn host_of(address: &str) -> String {
    crate::history::host(address).unwrap_or_else(|| address.to_owned())
}

/// The address bar. It says what's shown, and with a page shown a click on it is ⌘L: the
/// launcher opens on the page, its address in the field to change (`Shell::open_address`). With
/// no page there's no address, and a click does nothing. While a page's own address field has
/// the keyboard, as a new tab's start page gives it, the bar is that field, the page having no
/// toolbar in this layout.
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
        let Shown { said, page, pages } = &self.shown;
        let look = Look::of(cx);
        let p = look.p;
        let (text, placeholder) = said.clone();
        let editing = (pages.iter())
            .map(|page| page.read(cx).address_field().clone())
            .find(|field| field.read(cx).focus_handle(cx).is_focused(window));
        let (hovered, hover) = crate::theme::hover::eased(
            "arc-address",
            crate::browser::SITE_BUTTONS_FADE,
            window,
            cx,
        );
        let ends = page.as_ref().filter(|_| editing.is_none()).and_then(|page| {
            let pinned = crate::browser::pinned_extensions(page, cx);
            crate::browser::site_buttons(page.read(cx), page, p.text.opacity(0.7), hovered, pinned, cx)
        });
        div()
            .id("arc-address")
            .on_hover(hover)
            .debug_selector(|| "arc-address".to_owned())
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
            .map(|bar| match editing {
                Some(field) => bar.cursor_text().key_context("BrowserAddress").child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(Input::new(&field).appearance(false).small()),
                ),
                None => bar
                    .child(div().min_w_0().flex_1().truncate().child(text))
                    .when(page.is_some(), |bar| {
                        bar.cursor_text().on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenAddress), cx);
                        })
                    })
                    .children(ends),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::host_of;

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

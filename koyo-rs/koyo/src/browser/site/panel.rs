//! The site controls' popover (`docs/browser.md` 12.2): a row of the page's actions, what its
//! site is allowed and blocked from, and how it's connected.

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, Context, Div, ElementId, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Render, SharedString, Size, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div, point,
    px, size,
};
use koyo_browser_ipc::{Permission, SiteSetting};

use super::share;
use crate::browser::extensions::{self, Shown};
use crate::browser::view::BrowserView;
use crate::menu::{self, MenuBody};
use crate::shell::icon;
use crate::theme::{self, Palette};

/// The popover: how wide its content is, the room round it inside the menu's own padding, and
/// its parts' heights.
const WIDTH: f32 = 224.;
const PAD: f32 = 6.;
const ACTION: f32 = 32.;
const TITLE: f32 = 18.;
const ROW: f32 = 40.;
const ROUND: f32 = 34.;
const FOOT: f32 = 28.;
/// Between its parts.
const APART: f32 = 12.;
/// An extension's button in the row of them, the gap between two, and how many fit a line.
const EXTENSION: f32 = 26.;
const EXTENSION_GAP: f32 = 4.;
const PER_LINE: usize = 7;

/// The popover's colours. It lies over the sidebar and the page both, so it's its own glass,
/// dark or light with the theme and thicker than a menu's: what's behind tints it, as in Arc,
/// and what's on it still reads over a white page.
#[derive(Clone, Copy)]
pub(super) struct Tones {
    /// The glass itself.
    pub(super) glass: Hsla,
    /// An action's button, and its edge.
    pub(super) button: Hsla,
    pub(super) edge: Hsla,
    /// Under the pointer.
    pub(super) hover: Hsla,
    /// The round of a setting that's allowed and what's drawn on it; of one that's blocked.
    pub(super) on: Hsla,
    pub(super) on_mark: Hsla,
    pub(super) off: Hsla,
    pub(super) text: Hsla,
    pub(super) muted: Hsla,
}

impl Tones {
    pub(super) fn of(p: Palette) -> Tones {
        // The theme's text says which it is: light on a dark one.
        if p.text.l > 0.5 {
            Tones {
                glass: theme::gray(0.2, 0.62),
                button: theme::gray(0.05, 0.62),
                edge: theme::gray(1., 0.1),
                hover: theme::gray(1., 0.1),
                on: theme::gray(1., 0.95),
                on_mark: theme::gray(0.1, 1.),
                off: theme::gray(1., 0.16),
                text: theme::gray(1., 0.95),
                muted: theme::gray(1., 0.55),
            }
        } else {
            Tones {
                glass: theme::gray(0.96, 0.74),
                button: theme::gray(1., 0.85),
                edge: theme::gray(0., 0.1),
                hover: theme::gray(0., 0.06),
                on: theme::gray(0.12, 1.),
                on_mark: theme::gray(1., 1.),
                off: theme::gray(0., 0.1),
                text: theme::gray(0.08, 1.),
                muted: theme::gray(0., 0.55),
            }
        }
    }
}

/// The popover's content.
pub(super) struct SitePanel {
    page: WeakEntity<BrowserView>,
    /// The window the page is in, which its actions happen in.
    parent: AnyWindowHandle,
    /// The button it opened from, which the share sheet opens from too.
    at: menu::Trigger,
    /// What it lists: the site's settings as the host last said.
    listed: Vec<SiteSetting>,
    _page_changed: Option<Subscription>,
}

/// An action of the popover's top row.
#[derive(Clone, Copy)]
enum Act {
    Share,
    Picture,
    Find,
    Tools,
}

impl SitePanel {
    pub(super) fn new(
        page: WeakEntity<BrowserView>,
        parent: AnyWindowHandle,
        at: menu::Trigger,
        cx: &mut Context<Self>,
    ) -> Self {
        // Drawn again as the host says how the site's settings stand, and only then: a page's
        // view changes far more often, with its cursor and its tone.
        let page_changed = page.upgrade().map(|page| {
            cx.observe(&page, |panel: &mut Self, page, cx| {
                let listed = page.read(cx).site.clone().unwrap_or_default();
                if panel.listed != listed {
                    panel.listed = listed;
                    cx.notify();
                }
            })
        });
        let listed = (page.upgrade())
            .and_then(|page| page.read(cx).site.clone())
            .unwrap_or_default();
        SitePanel {
            page,
            parent,
            at,
            listed,
            _page_changed: page_changed,
        }
    }

    fn settings(&self, _: &App) -> Vec<SiteSetting> {
        self.listed.clone()
    }

    /// Does `act` on the page, in its window, and closes the popover.
    fn act(&self, act: Act, cx: &mut App) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        let at = self.at.drawn();
        let _ = self.parent.update(cx, |_, window, cx| match act {
            Act::Share => share(&page.read(cx).address(), at, window, cx),
            Act::Picture => page.update(cx, BrowserView::copy_picture),
            Act::Find => page.update(cx, |page, cx| page.open_find(window, cx)),
            Act::Tools => page.update(cx, |page, cx| page.inspect(None, cx)),
        });
        self.at.close(cx);
    }

    fn actions(&self, t: Tones, cx: &mut Context<Self>) -> Div {
        let acts = [
            ("site-share", IconName::Share, "Share", Act::Share),
            ("site-picture", IconName::Camera, "Copy a picture of the page", Act::Picture),
            ("site-find", IconName::Search, "Find in page", Act::Find),
            ("site-tools", IconName::Inspector, "Developer tools", Act::Tools),
        ];
        let row = div().h(px(ACTION)).flex().gap(px(8.));
        row.children(acts.map(|(id, mark, says, act)| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .h_full()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .bg(t.button)
                .border_1()
                .border_color(t.edge)
                .cursor_pointer()
                .hover(move |button| button.border_color(t.muted))
                .tooltip(move |window, cx| Tooltip::new(says).build(window, cx))
                .child(icon(mark, 16.).text_color(t.text))
                .on_click(cx.listener(move |this, _, _, cx| this.act(act, cx)))
        }))
    }

    /// Runs `extension`'s action on the page, as a press on its icon in Chrome does, and
    /// closes the popover: a popup it opens hangs from the button the popover opened from.
    fn run(&self, extension: &str, cx: &mut App) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        let at = self.at.drawn();
        let _ = self.parent.update(cx, |_, window, cx| {
            page.update(cx, |page, cx| page.extension_act(extension, at, window, cx));
        });
        self.at.close(cx);
    }

    /// Closes the popover and opens `shown`'s menu in the page's window, under the button
    /// the popover opened from.
    fn menu(&self, shown: &Shown, cx: &mut App) {
        let Some(page) = self.page.upgrade() else {
            return;
        };
        let at = self.at.drawn();
        self.at.close(cx);
        let _ = self.parent.update(cx, |_, window, cx| {
            let under = point(at.left(), at.bottom());
            page.update(cx, |page, cx| page.extension_menu(shown, under, window, cx));
        });
    }

    /// A button of the extensions' row.
    fn extension_button(id: (&'static str, usize), t: Tones) -> Stateful<Div> {
        div()
            .id(id)
            .size(px(EXTENSION))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(move |button| button.bg(t.hover))
    }

    /// The row of the profile's extensions (`docs/extensions.md` 5): an icon each, pinned
    /// ones first, then "+", which goes to the Chrome Web Store. None where the engine runs
    /// no extensions with icons: off-screen pages, or CEF's own framework.
    fn extensions(&self, t: Tones, cx: &mut Context<Self>) -> Option<Div> {
        let page = self.page.upgrade()?;
        page.read(cx).extension_count(cx)?;
        let shown = page.update(cx, |page, cx| page.extensions(false, cx));
        let icons = shown.into_iter().enumerate().map(|(index, shown)| {
            let says = shown.title.clone();
            let (pressed, asked) = (shown.id.clone(), shown.clone());
            Self::extension_button(("site-extension", index), t)
                .debug_selector(move || format!("site-extension-{index}"))
                .tooltip(move |window, cx| Tooltip::new(says.clone()).build(window, cx))
                .child(extensions::icon::face(&shown, t.text, t.off))
                .on_click(cx.listener(move |this, _, _, cx| this.run(&pressed, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, _, cx| this.menu(&asked, cx)),
                )
        });
        let add = Self::extension_button(("site-extension-add", 0), t)
            .debug_selector(|| "site-extension-add".to_owned())
            .tooltip(|window, cx| Tooltip::new("Add extensions").build(window, cx))
            .child(icon(IconName::Plus, 15.).text_color(t.muted))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(page) = this.page.upgrade() {
                    page.update(cx, BrowserView::open_extension_store);
                }
                this.at.close(cx);
            }));
        let row = div()
            .debug_selector(|| "site-extensions".to_owned())
            .mt(px(APART))
            .flex()
            .flex_wrap()
            .gap(px(EXTENSION_GAP));
        Some(row.children(icons).child(add))
    }

    fn setting(&self, setting: SiteSetting, t: Tones) -> Stateful<Div> {
        let SiteSetting { permission, allowed } = setting;
        let (mark, name) = shown(permission);
        let page = self.page.clone();
        let round = div()
            .size(px(ROUND))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .map(|round| {
                if allowed {
                    round.bg(t.on).child(icon(mark, 16.).text_color(t.on_mark))
                } else {
                    round.bg(t.off).child(icon(mark, 16.).text_color(t.muted))
                }
            });
        let state = if allowed { "Allowed" } else { "Blocked" };
        div()
            .id(ElementId::Name(format!("site-setting-{name}").into()))
            .debug_selector(move || format!("site-setting-{name}"))
            .h(px(ROW))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .px(px(4.))
            .mx(px(-4.))
            .hover(move |row| row.bg(t.hover))
            .child(round)
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).truncate().child(name))
                    .child(div().text_size(px(11.)).text_color(t.muted).child(state)),
            )
            .on_click(move |_, _, cx| {
                let _ = page.update(cx, |page, cx| page.set_site_setting(permission, !allowed, cx));
            })
    }

    fn foot(&self, t: Tones, cx: &App) -> Div {
        let address = (self.page.upgrade()).map(|page| page.read(cx).address()).unwrap_or_default();
        let (mark, says): (_, SharedString) = if address.starts_with("https://") {
            (IconName::Lock, "Secure".into())
        } else {
            (IconName::LockOpen, "Not Secure".into())
        };
        let connection = div()
            .debug_selector(|| "site-connection".to_owned())
            .h(px(FOOT))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(8.))
            .bg(t.hover)
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(icon(mark, 13.).text_color(t.text))
            .child(says);
        div()
            .mt(px(8.))
            .pt(px(8.))
            .border_t_1()
            .border_color(t.edge)
            .flex()
            .child(connection)
    }
}

/// How a permission is shown: its icon and its name.
pub(super) fn shown(permission: Permission) -> (IconName, &'static str) {
    match permission {
        Permission::Camera => (IconName::Camera, "Camera"),
        Permission::Microphone => (IconName::Mic, "Microphone"),
        Permission::Location => (IconName::MapPin, "Location"),
        Permission::Notifications => (IconName::Bell, "Notifications"),
        Permission::Clipboard => (IconName::Clipboard, "Clipboard"),
        Permission::Midi => (IconName::KeyboardMusic, "MIDI Devices"),
        Permission::Fonts => (IconName::Type, "Fonts"),
        Permission::Windows => (IconName::AppWindow, "Window Placement"),
        Permission::Downloads => (IconName::Download, "Downloads"),
        Permission::Screen => (IconName::Monitor, "Screen"),
        Permission::Sensors | Permission::LocalNetwork | Permission::Files | Permission::Other => {
            (IconName::Settings2, "Other")
        }
    }
}

/// How tall the popover's content is with `settings` rows and, where extensions have their
/// row, the icons of `extensions` of them and the "+" after.
fn height(settings: usize, extensions: Option<usize>) -> f32 {
    let listed = if settings == 0 {
        0.
    } else {
        APART + TITLE + 4. + ROW * settings as f32
    };
    let row = extensions.map_or(0., |icons| {
        let lines = (icons + 1).div_ceil(PER_LINE) as f32;
        APART + lines * EXTENSION + (lines - 1.) * EXTENSION_GAP
    });
    PAD + ACTION + row + listed + 8. + 1. + 8. + FOOT + PAD
}

impl MenuBody for SitePanel {
    fn hears(&mut self, _: &str, _: &mut Context<Self>) -> bool {
        false
    }

    fn extent(&self, _: &Window, cx: &App) -> Size<Pixels> {
        let extensions = (self.page.upgrade()).and_then(|page| page.read(cx).extension_count(cx));
        size(px(WIDTH), px(height(self.settings(cx).len(), extensions)))
    }

    fn backdrop(&self, cx: &App) -> Option<Hsla> {
        Some(Tones::of(theme::palette(cx)).glass)
    }

    fn callout(&self) -> bool {
        true
    }
}

impl Render for SitePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Tones::of(theme::palette(cx));
        let settings = self.settings(cx);
        let listed = (!settings.is_empty()).then(|| {
            let title = div()
                .h(px(TITLE))
                .mt(px(APART))
                .mb(px(4.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Settings");
            let rows = settings.into_iter().map(|setting| self.setting(setting, t));
            div().flex().flex_col().child(title).children(rows)
        });
        div()
            .debug_selector(|| "site-panel".to_owned())
            .size_full()
            .p(px(PAD))
            .flex()
            .flex_col()
            .text_color(t.text)
            .child(self.actions(t, cx))
            .children(self.extensions(t, cx))
            .children(listed)
            .child(self.foot(t, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popover_is_as_tall_as_what_it_lists() {
        let bare = height(0, None);
        assert!((height(2, None) - bare - (APART + TITLE + 4. + 2. * ROW)).abs() < f32::EPSILON);
        // The top row and the foot are there with nothing listed.
        assert!(bare > ACTION + FOOT);
    }

    #[test]
    fn the_extensions_row_takes_a_line_for_every_seven_buttons() {
        let bare = height(0, None);
        let line = |lines: f32| APART + lines * EXTENSION + (lines - 1.) * EXTENSION_GAP;
        // With no extension there's the "+" alone; six and it fill a line, seven start another.
        assert!((height(0, Some(0)) - bare - line(1.)).abs() < f32::EPSILON);
        assert!((height(0, Some(6)) - bare - line(1.)).abs() < f32::EPSILON);
        assert!((height(0, Some(7)) - bare - line(2.)).abs() < f32::EPSILON);
        // Seven buttons and the gaps between fit the popover's width.
        assert!(PER_LINE as f32 * EXTENSION + (PER_LINE - 1) as f32 * EXTENSION_GAP <= WIDTH - 2. * PAD);
    }

    #[test]
    fn every_permission_the_host_lists_has_a_name_of_its_own() {
        let listed = [
            Permission::Camera,
            Permission::Microphone,
            Permission::Location,
            Permission::Notifications,
            Permission::Clipboard,
            Permission::Midi,
            Permission::Fonts,
            Permission::Windows,
            Permission::Downloads,
        ];
        let names: std::collections::HashSet<&str> = listed.map(|one| shown(one).1).into();
        assert_eq!(names.len(), listed.len());
        assert!(!names.contains("Other"));
    }
}

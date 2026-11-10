//! The toolbar over a page: history, reload, the address, responsive mode and the inspector;
//! none in the Arc layout, whose sidebar has them. And what's shown over the page itself: the
//! start page, a notice when the engine or the page failed, and the bar that says who has the
//! page when it's contested.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Context, Div, ElementId, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, Window, div, point, px, relative, size,
};
use koyo_browser_ipc::{Go, PageState};

use super::control::Mode;
use super::host::HostStatus;
use super::lightness::glass_dark;
use super::view::{BrowserView, Notice};
use crate::held::{self, Held};
use crate::responsive::bar::toggle_button;
use crate::shell::{icon, icon_button};
use crate::theme::{self, Palette};

/// Back, or forward: a click goes a step; held, or pressed with the right button, it lists
/// the pages that way.
fn history_button(back: bool, enabled: bool, cx: &mut Context<BrowserView>) -> Div {
    let (id, glyph, name, to) = if back {
        ("browser-back", IconName::ArrowLeft, "Back", Go::Back)
    } else {
        (
            "browser-forward",
            IconName::ArrowRight,
            "Forward",
            Go::Forward,
        )
    };
    let button = icon_button(id, glyph, name, cx)
        .small()
        .disabled(!enabled)
        .on_click(cx.listener(move |this, _, _, cx| {
            if !this.ends_history_hold() {
                this.go(to, cx);
            }
        }));
    super::history::button(button, back, enabled, cx)
}

/// The toolbar: history, reload, the address and the inspector, lit while the shell shows the
/// `DevTools`.
pub(super) fn toolbar(view: &BrowserView, window: &Window, cx: &mut Context<BrowserView>) -> Div {
    let p = theme::palette(cx);
    let page = &view.page;
    let (open, controls) = (view.is_open(cx), view.controls(cx));
    let editing = view.address.read(cx).focus_handle(cx).is_focused(window);
    let (loading, inspecting) = (controls.loading, view.devtools.is_some());
    div()
        .relative()
        .h(px(40.))
        .px_2()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(2.))
        .border_b_1()
        .border_color(p.hairline)
        .children(progress(page, p))
        .child(history_button(true, controls.back, cx))
        .child(history_button(false, controls.forward, cx))
        .child(
            icon_button(
                "browser-reload",
                if loading {
                    IconName::X
                } else {
                    IconName::RefreshCw
                },
                if loading { "Stop" } else { "Reload" },
                cx,
            )
            .small()
            .disabled(!controls.reload)
            .on_click(cx.listener(|this, _, _, cx| this.reload_or_stop(cx))),
        )
        .child(address(view, editing, p))
        .children(view.zoom_chip(cx))
        .child(
            toggle_button(
                "browser-responsive",
                view.responsive.mode.is_some(),
                &super::ToggleResponsive,
                "BrowserPage",
                cx,
            )
            .disabled(view.is_start_page())
            .on_click(cx.listener(|this, _, _, cx| this.toggle_responsive(cx))),
        )
        .child(
            icon_button(
                "browser-inspect",
                IconName::Inspector,
                if inspecting {
                    "Hide DevTools"
                } else {
                    "Show DevTools"
                },
                cx,
            )
            .small()
            .selected(inspecting)
            .disabled(!(open || inspecting))
            .on_click(cx.listener(|this, _, _, cx| this.inspect(None, cx))),
        )
}

/// A line over the toolbar's foot while the page loads, as long as the share loaded.
fn progress(page: &PageState, p: Palette) -> Option<Div> {
    page.loading.then(|| {
        // Some of it shows at once, before the engine says how far it got.
        let share = f32::from(page.progress.max(8)) / 100.;
        div()
            .debug_selector(|| "browser-progress".to_owned())
            .absolute()
            .left_0()
            .bottom(px(-1.))
            .h(px(2.))
            .w(relative(share))
            .bg(p.accent)
    })
}

/// The address field, with a lock while a secure page shows in it.
fn address(view: &BrowserView, editing: bool, p: Palette) -> Div {
    let secure = view.page.url.starts_with("https://");
    div()
        .key_context("BrowserAddress")
        .ml_1()
        .h(px(26.))
        .min_w_0()
        .flex_1()
        .flex()
        .items_center()
        .gap(px(6.))
        .px_2()
        .rounded(px(8.))
        .bg(p.raised)
        .border_1()
        .border_color(if editing { p.accent } else { p.hairline })
        .when(secure && !editing, |field| {
            field.child(icon(IconName::Lock, 12.).text_color(p.muted))
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(Input::new(&view.address).appearance(false).small()),
        )
}

/// The bar at the bottom of the page while it's contested: an agent using it, with the
/// person's way to take it; or the person having taken it from an agent, with the way to
/// give it back. Liquid Glass where the system has it, dark over a light page and light over a
/// dark one (`theme::glass`).
pub(super) fn control_bar(
    view: &mut BrowserView,
    window: &mut Window,
    cx: &mut Context<BrowserView>,
) -> Option<AnyElement> {
    let (text, id, label, on_click): (_, _, _, OnClick) = if let Some(agent) = view.control.agent()
    {
        let take = cx.listener(|this, _: &ClickEvent, _, cx| this.person_acts(Mode::Explicit, cx));
        let text = format!("{} is using this tab", agent.label);
        (text, "browser-take-control", "Take control", Box::new(take))
    } else {
        let displaced = view.control.displaced.as_ref()?;
        let give_back = cx.listener(|this, _: &ClickEvent, _, cx| this.give_back(cx));
        // A page the person is only using goes back by itself once they stop.
        let text = if view.control.person_using() {
            format!("{} waits while you use this tab", displaced.label)
        } else {
            format!("You took over this tab from {}", displaced.label)
        };
        (text, "browser-give-back", "Give back", Box::new(give_back))
    };
    let dark = view.read_bar_dark();
    let glass = ElementId::NamedInteger("browser-control-glass".into(), view.pane);
    let held = Held {
        text,
        id,
        label,
        on_click,
    };
    Some(held::bar(glass, held, dark, window, cx))
}

impl BrowserView {
    /// Whether the control bar's glass is dark, the page under it read again (`glass_dark`), as
    /// it is each time the bar is drawn; none while its tone is the theme's.
    pub(super) fn read_bar_dark(&mut self) -> Option<bool> {
        self.bar_dark = self.page_bounds().and_then(|page| {
            let width = page.size.width.min(px(held::WIDTH));
            let origin = point(
                page.center().x - width / 2.,
                page.bottom() - px(held::BOTTOM + held::HEIGHT),
            );
            let region = Bounds::new(origin, size(width, px(held::HEIGHT)));
            glass_dark([&*self], region, self.bar_dark)
        });
        self.bar_dark
    }
}

/// What a notice's button does.
type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// What covers the page when it can't be shown: the engine failed or is starting, or the
/// page stopped.
pub(super) fn overlay(
    view: &BrowserView,
    status: &HostStatus,
    cx: &mut Context<BrowserView>,
) -> Option<AnyElement> {
    let start = view.is_start_page();
    let p = theme::palette(cx);
    let (message, action): (String, Option<(&str, OnClick)>) = match (&view.notice, status) {
        (_, HostStatus::Failed(message)) if !start => {
            let restart = cx.listener(|this, _: &ClickEvent, _, cx| this.restart_host(cx));
            (
                message.clone(),
                Some(("Restart browser", Box::new(restart))),
            )
        }
        (Some(Notice::Gone(reason)), _) => {
            let reload = cx.listener(|this, _: &ClickEvent, _, cx| this.go(Go::Reload, cx));
            (
                format!("This page stopped: {reason}"),
                Some(("Reload", Box::new(reload))),
            )
        }
        (Some(Notice::Failed(message)), _) => (message.clone(), None),
        // A host starting quickly leaves the page blank, as it would be while loading.
        (None, HostStatus::Starting) if !start && view.starting_long(cx) => {
            ("Starting the browser…".to_owned(), None)
        }
        _ => return None,
    };
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(p.floating())
            // The page under it stays out of reach.
            .occlude()
            .child(
                div()
                    .max_w(px(420.))
                    .text_center()
                    .text_sm()
                    .text_color(p.muted)
                    .child(message),
            )
            .children(action.map(|(label, on_click)| {
                held::button("browser-notice-action", label, false, p)
                    .text_sm()
                    .text_color(p.text)
                    .on_click(on_click)
            }))
            .into_any_element(),
    )
}

/// What a new tab shows until an address is entered.
pub(super) fn start_page(p: Palette) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .child(icon(IconName::Globe, 28.).text_color(p.muted))
        .child(
            div()
                .text_sm()
                .text_color(p.muted)
                .child("Type an address or a search above"),
        )
}

/// What the strip calls a page: its title, else its address.
pub fn tab_title(title: &str, url: &str) -> String {
    if !title.is_empty() {
        title.to_owned()
    } else if url.is_empty() {
        crate::model::PaneKind::Browser.new_tab_title().into()
    } else {
        display_url(url)
    }
}

/// An address as the strip shows it: no scheme, no trailing slash.
pub fn display_url(url: &str) -> String {
    let rest = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .trim_start_matches("www.");
    rest.strip_suffix('/').unwrap_or(rest).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

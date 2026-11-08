//! The top of the Arc sidebar (`docs/arc-sidebar.md` 1, 2): back, forward and reload for the
//! page shown, at the sidebar's edge of the top rail, and under them the address bar, which says
//! what's shown: a page's address, a terminal's program, a chat's name, "File" or "Settings".

use gpui_kit::Focusable as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::Look;
use super::actions::{PageBack, PageForward, PageReload};
use crate::model::PaneKind;
use crate::navigation::Route;
use crate::shell::{Shell, action_button};

/// Back, forward and reload, for the page shown; greyed out with no page shown.
pub fn controls(shell: &Shell, cx: &App) -> Div {
    let controls = shell.page_controls(cx).unwrap_or_default();
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
        .child(action_button("arc-reload", reload, said, PageReload, cx).disabled(!controls.reload))
}

/// What the address bar says, and whether it's a placeholder, said dimmer.
pub(super) fn said(shell: &Shell, cx: &App) -> (SharedString, bool) {
    if matches!(shell.route(cx), Route::Settings(_)) {
        return ("Settings".into(), false);
    }
    let Some(tab) = shell.workspace.tab_shown() else {
        return ("No tab open".into(), true);
    };
    let pane = tab.first();
    match pane.kind {
        PaneKind::Browser => {
            let address = pane.address(cx).unwrap_or_default();
            if address.is_empty() {
                ("Search or enter address".into(), true)
            } else {
                (host_of(&address).into(), false)
            }
        }
        PaneKind::Terminal | PaneKind::Chat => (pane.title.clone(), false),
        PaneKind::Editor => ("File".into(), false),
    }
}

/// A web address as Arc shows it: its host, less `www.`; anything else as it is.
fn host_of(address: &str) -> String {
    crate::history::host(address).unwrap_or_else(|| address.to_owned())
}

/// The address bar. With a page shown it's the page's own address field, the page having no
/// toolbar in this layout: its host until it's clicked, or ⌘L or a new tab puts the keyboard
/// in it, and then the whole address, to edit and enter.
pub fn address_bar(
    shell: &Shell,
    look: Look,
    window: &Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let p = look.p;
    let (text, placeholder) = said(shell, cx);
    let page = shell.shown_page(cx);
    let editing = (page.as_ref())
        .map(|page| page.read(cx).address_field().clone())
        .filter(|field| field.read(cx).focus_handle(cx).is_focused(window));
    div()
        .id("arc-address")
        .debug_selector(|| "arc-address".to_owned())
        .h(px(34.))
        .flex_shrink_0()
        .px(px(12.))
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
            // The page's keys for its address, ⌘R and the rest, go with it.
            Some(field) => bar.key_context("BrowserAddress").child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(Input::new(&field).appearance(false).small()),
            ),
            None => bar.child(div().min_w_0().truncate().child(text)),
        })
        .when_some(page, |bar, page| {
            bar.cursor_text().on_click(move |_, window, cx| {
                page.update(cx, |page, cx| page.focus_address(window, cx));
            })
        })
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

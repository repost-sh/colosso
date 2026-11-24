//! The pinned extensions' icons where a layout has no toolbar for them (`docs/extensions.md`
//! 5): in the sidebar's address field, before the link and site-controls buttons, there
//! while those are (`site::buttons`).

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::base::ElementExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Bounds, Entity, MouseButton, MouseDownEvent, div, px};

use super::icon;
use crate::browser::BrowserView;
use crate::theme;

/// The icons of the extensions pinned in the profile of `view`'s page, in pin order, each
/// with its badge: a press runs one's action, its popup hanging from under the icon, and a
/// right press opens its menu. None for the developer tools, or a page that isn't open.
pub fn icons(view: &Entity<BrowserView>, cx: &App) -> Vec<AnyElement> {
    let p = theme::palette(cx);
    let page = view.read(cx);
    let shown = if page.is_inspector() { Vec::new() } else { page.extensions(true, cx) };
    let icons = shown.into_iter().enumerate().map(|(index, shown)| {
        let says = shown.title.clone();
        let (pressed, asked) = (shown.id.clone(), shown.clone());
        let drawn = Rc::new(Cell::new(Bounds::default()));
        let measured = drawn.clone();
        let (acts, asks, downs) = (view.clone(), view.clone(), view.clone());
        let held = pressed.clone();
        div()
            .id(("pinned-extension", index))
            .debug_selector(move || format!("pinned-extension-{index}"))
            .size(px(icon::BUTTON))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |button| button.bg(p.hover))
            .tooltip(move |window, cx| Tooltip::new(says.clone()).build(window, cx))
            .child(icon::face(&shown, p.text, p.selected).on_prepaint(move |bounds, _, _| measured.set(bounds)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                downs.update(cx, |page, cx| page.extension_pointer_down(&held, cx));
            })
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                acts.update(cx, |page, cx| page.extension_act(&pressed, drawn.get(), window, cx));
            })
            .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                asks.update(cx, |page, cx| page.extension_menu(&asked, event.position, window, cx));
            })
            .into_any_element()
    });
    icons.collect()
}

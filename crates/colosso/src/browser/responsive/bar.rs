//! A page's device bar, between the toolbar and the page while responsive mode is on: the
//! shared bar (`crate::responsive::bar`) with the device table as its presets, sizes in
//! logical pixels, and a reload offered once the user agent changed on a loaded page.

use std::rc::Rc;

use colosso_browser_ipc::{Go, PaneId};
use gpui_kit::{
    Action, App, ClickEvent, Context, Div, InteractiveElement as _, IntoElement as _,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use super::devices::DEVICES;
use super::{Change, SetBy, Zoom};
use crate::browser::view::BrowserView;
use crate::responsive::bar::{Bar, Fields};
use crate::theme::{self, Palette};

pub use crate::responsive::bar::HEIGHT;

/// A device picked from the bar's menu for the page `pane`: a preset by its place in the
/// table, or `None` for a free size.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct PickDevice {
    pub pane: PaneId,
    pub device: Option<usize>,
}

/// A zoom picked from the bar's menu for the page `pane`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct PickZoom {
    pub pane: PaneId,
    pub zoom: Zoom,
}

/// The device bar, while the mode is on.
pub fn device_bar(
    view: &mut BrowserView,
    window: &mut Window,
    cx: &mut Context<BrowserView>,
) -> Option<Div> {
    let mode = view.responsive.mode.clone()?;
    if view.responsive.fields.is_none() {
        view.responsive.fields = Some(Fields::new(BrowserView::size_typed, window, cx));
    }
    view.show_size(false, window, cx);
    let fields = view.responsive.fields.as_ref()?;
    let pane = view.pane;
    let me = cx.entity().downgrade();
    let restore = Rc::new(move |window: &mut Window, cx: &mut App| {
        let _ = me.update(cx, |this, cx| {
            this.show_size(true, window, cx);
            window.focus(&this.focus, cx);
        });
    });
    let rotate = cx.listener(|this, _: &ClickEvent, _, cx| {
        let landscape = (this.responsive.mode.as_ref()).is_some_and(|mode| !mode.landscape());
        let change = Change {
            landscape: Some(landscape),
            ..Change::default()
        };
        this.person_changes(&change, cx);
    });
    let close = cx.listener(|this, _: &ClickEvent, _, cx| this.set_responsive(None, cx));
    let reload = (view.responsive.reload_hint)
        .then(|| reload_hint(theme::palette(cx), cx).into_any_element());
    let bar = Bar {
        ids: "browser",
        agent: agent_of(&mode.set_by),
        presets: DEVICES.iter().map(|device| device.name.into()).collect(),
        preset: mode
            .device
            .and_then(|device| DEVICES.iter().position(|row| row.id == device.id)),
        pick_preset: Rc::new(move |device| Box::new(PickDevice { pane, device })),
        fields,
        zoom: mode.zoom,
        shown_percent: view.shown_percent(),
        pick_zoom: Rc::new(move |zoom| Box::new(PickZoom { pane, zoom })),
        restore,
        rotate: Rc::new(rotate),
        extra: reload,
        close: Rc::new(close),
    };
    Some(bar.render(cx))
}

/// The agent's name when an agent set the size.
fn agent_of(set_by: &SetBy) -> Option<String> {
    match set_by {
        SetBy::Agent(agent) => Some(agent.clone()),
        SetBy::Person => None,
    }
}

/// The offer to reload the page, for a site that picks its pages by the user agent.
fn reload_hint(p: Palette, cx: &mut Context<BrowserView>) -> Div {
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_1()
        .text_color(p.muted)
        .child("Some sites switch only once the page loads again.")
        .child(
            div()
                .id("browser-responsive-reload")
                .text_color(p.accent)
                .cursor_pointer()
                .child("Reload")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.responsive.reload_hint = false;
                    this.go(Go::Reload, cx);
                })),
        )
}

impl BrowserView {
    /// Shows the page's size in the fields, those being edited only when `restore`.
    fn show_size(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(mode), Some(fields)) = (&self.responsive.mode, &self.responsive.fields) else {
            return;
        };
        fields.show(mode.width, mode.height, restore, window, cx);
    }

    /// Enter in a field: the page takes the size typed, or shows its own again.
    fn size_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = (self.responsive.fields.as_ref()).and_then(|fields| fields.typed(cx));
        if let Some((width, height)) = typed {
            self.person_sizes(width, height, cx);
        }
        self.show_size(true, window, cx);
    }
}

/// What the window does with the bar's menus' picks: routes them to the page they name.
pub fn handle_actions(root: Div) -> Div {
    root.on_action(|pick: &PickDevice, _, cx| {
        crate::browser::menu::with_view(pick.pane, cx, |view, cx| {
            view.pick_device(pick.device.and_then(|at| DEVICES.get(at)), cx);
        });
    })
    .on_action(|pick: &PickZoom, _, cx| {
        crate::browser::menu::with_view(pick.pane, cx, |view, cx| {
            let change = Change {
                zoom: Some(pick.zoom),
                ..Change::default()
            };
            view.person_changes(&change, cx);
        });
    })
}

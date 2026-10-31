//! A page's `DevTools`: in the page's pane at its bottom, left or right, past a divider the
//! person drags, or detached into a pane of their own beside it in the tab. Where the last
//! ones went is where the next ones open, and the docked size holds for every page.

use colosso_browser_ipc as ipc;
use gpui_kit::assets::IconName;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, AppContext as _, Context, Div, DragMoveEvent, Entity,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseUpEvent,
    ParentElement as _, Render, Stateful, Styled, Subscription, Window, deferred, div, px,
};
use serde::{Deserialize, Serialize};

use super::find::ACTUAL_SIZE;
use super::view::{BrowserEvent, BrowserView};
use super::{ActualSize, FindInPage, FindNext, FindPrevious, ZoomIn, ZoomOut};
use super::{input, toolbar};
use crate::settings;
use crate::shell::{ellipsized, icon, icon_button};
use crate::theme::{self, Palette};

/// Where a page's `DevTools` are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dock {
    #[default]
    Bottom,
    Left,
    Right,
    /// In a pane of their own beside the page's.
    Detached,
}

impl Dock {
    pub const ALL: [Dock; 4] = [Dock::Bottom, Dock::Left, Dock::Right, Dock::Detached];

    pub fn label(self) -> &'static str {
        match self {
            Dock::Bottom => "At the bottom of the page",
            Dock::Left => "Left of the page",
            Dock::Right => "Right of the page",
            Dock::Detached => "In a pane of their own",
        }
    }

    /// A menu of where `DevTools` can open, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        settings::checked_menu(Dock::ALL, self, Dock::label, |dock| {
            Box::new(SetDevtoolsDock(dock))
        })
    }
}

/// Picks where `DevTools` open, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct SetDevtoolsDock(pub Dock);

/// Where `DevTools` open, and how much room they take docked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DevtoolsPlace {
    pub dock: Dock,
    /// Their height at the bottom of a page.
    pub height: f32,
    /// Their width at a side of a page.
    pub width: f32,
}

impl Default for DevtoolsPlace {
    fn default() -> Self {
        DevtoolsPlace {
            dock: Dock::Bottom,
            height: 320.,
            width: 440.,
        }
    }
}

/// The least room a page and its docked `DevTools` keep when the divider between them moves.
pub const DOCK_MIN: f32 = 120.;

/// A page's open `DevTools`, and where they are.
pub(super) struct Devtools {
    pub(super) view: Entity<BrowserView>,
    pub(super) dock: Dock,
    _closed: Subscription,
}

/// `DevTools` asked for and not made yet, and the point of the page to reveal in them.
pub(super) struct Asked {
    at: Option<ipc::Point>,
}

/// The divider between a page and its docked `DevTools` in hand, by the page's id.
#[derive(Clone)]
pub struct DockDivider(u64);

impl BrowserView {
    /// Shows the page's `DevTools`, revealing the element at `at` when there is one; asked
    /// without one while they show, hides them.
    pub fn inspect(&mut self, at: Option<ipc::Point>, cx: &mut Context<Self>) {
        if self.is_inspector() {
            return;
        }
        match (&self.devtools, at) {
            (Some(tools), Some(at)) => tools.view.update(cx, |tools, cx| tools.reveal(at, cx)),
            (Some(_), None) => self.close_devtools(cx),
            (None, at) => {
                // They're made with the window, by the next render.
                self.open_devtools = Some(Asked { at });
                cx.notify();
            }
        }
    }

    /// Opens the `DevTools` asked for, where the last ones went.
    pub(super) fn make_devtools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Asked { at }) = self.open_devtools.take() else {
            return;
        };
        if self.devtools.is_some() {
            return;
        }
        let page = cx.entity();
        let view = cx.new(|cx| BrowserView::inspector(page, at, window, cx));
        let closed = cx.subscribe(&view, |this, tools, event, cx| {
            let ours = this
                .devtools
                .as_ref()
                .is_some_and(|open| open.view == tools);
            if matches!(event, BrowserEvent::Closed) && ours {
                this.close_devtools(cx);
            }
        });
        let dock = settings::get(cx).devtools.dock;
        self.devtools = Some(Devtools {
            view: view.clone(),
            dock,
            _closed: closed,
        });
        if dock == Dock::Detached {
            cx.emit(BrowserEvent::Detach(view));
        }
        cx.notify();
    }

    /// Closes the page's `DevTools`; detached, their pane goes with them.
    pub fn close_devtools(&mut self, cx: &mut Context<Self>) {
        let Some(tools) = self.devtools.take() else {
            return;
        };
        if tools.dock == Dock::Detached {
            cx.emit(BrowserEvent::Undock(tools.view));
        }
        cx.notify();
    }

    /// Moves the page's `DevTools` to `dock`, where the next ones open too.
    pub fn dock_devtools(&mut self, dock: Dock, cx: &mut Context<Self>) {
        settings::update(cx, |settings| settings.devtools.dock = dock);
        let Some(tools) = &mut self.devtools else {
            return;
        };
        let was = std::mem::replace(&mut tools.dock, dock);
        let view = tools.view.clone();
        if dock == Dock::Detached && was != dock {
            cx.emit(BrowserEvent::Detach(view));
        } else if was == Dock::Detached && dock != was {
            cx.emit(BrowserEvent::Undock(view));
        }
        cx.notify();
    }

    /// Where the page's `DevTools` are, while they're open.
    pub fn devtools_dock(&self) -> Option<Dock> {
        self.devtools.as_ref().map(|tools| tools.dock)
    }

    /// The page's open `DevTools`.
    pub fn devtools(&self) -> Option<&Entity<BrowserView>> {
        self.devtools.as_ref().map(|tools| &tools.view)
    }

    /// The page under the toolbar, with its `DevTools` beside it while they're docked.
    fn body(&mut self, page: Stateful<Div>, cx: &mut Context<Self>) -> Div {
        let body = div().flex_1().min_h_0().min_w_0().w_full().flex();
        let page = div()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(page)
            // Among the window's overlays: the page's native layer hides what's under it.
            .children(self.find_bar(cx).map(deferred));
        let docked = self
            .devtools
            .as_ref()
            .filter(|tools| tools.dock != Dock::Detached);
        let Some((tools, dock)) = docked.map(|tools| (tools.view.clone(), tools.dock)) else {
            return body.child(page);
        };
        let p = theme::palette(cx);
        let row = dock != Dock::Bottom;
        let place = &settings::get(cx).devtools;
        let size = (self.dock_drag).unwrap_or(if row { place.width } else { place.height });
        let me = cx.entity();
        let panel = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .min_w_0()
            .min_h_0()
            .map(|panel| {
                if row {
                    panel.w(px(size)).h_full()
                } else {
                    panel.h(px(size)).w_full()
                }
            })
            .child(bar(&me, &self.title(), dock, p, cx))
            .child(div().flex_1().min_h_0().min_w_0().child(tools));
        let id = self.pane;
        let divider = crate::shell::divider(
            ("devtools-divider", id),
            "devtools-divider".into(),
            row,
            DockDivider(id),
            p,
        );
        let parts: [AnyElement; 3] = if dock == Dock::Left {
            [
                panel.into_any_element(),
                divider.into_any_element(),
                page.into_any_element(),
            ]
        } else {
            [
                page.into_any_element(),
                divider.into_any_element(),
                panel.into_any_element(),
            ]
        };
        let let_go = |this: &mut Self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>| {
            this.let_go_of_divider(cx);
        };
        body.when(!row, Styled::flex_col)
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<DockDivider>, _, cx| {
                    if event.drag(cx).0 != id {
                        return;
                    }
                    let (bounds, at) = (event.bounds, event.event.position);
                    let (span, size) = match dock {
                        Dock::Bottom => (bounds.size.height, bounds.bottom() - at.y),
                        Dock::Right => (bounds.size.width, bounds.right() - at.x),
                        Dock::Left => (bounds.size.width, at.x - bounds.left()),
                        Dock::Detached => return,
                    };
                    let most = (span.as_f32() - DOCK_MIN).max(DOCK_MIN);
                    let size = size.as_f32().clamp(DOCK_MIN, most);
                    if this.dock_drag != Some(size) {
                        this.dock_drag = Some(size);
                        cx.notify();
                    }
                }),
            )
            // Let go wherever the pointer is.
            .capture_any_mouse_up(cx.listener(let_go))
            .on_mouse_up_out(MouseButton::Left, cx.listener(let_go))
            .children(parts)
    }

    /// The divider was let go: its place holds for every page.
    fn let_go_of_divider(&mut self, cx: &mut Context<Self>) {
        let Some(size) = self.dock_drag.take() else {
            return;
        };
        let row = matches!(self.devtools_dock(), Some(Dock::Left | Dock::Right));
        settings::update(cx, |settings| {
            if row {
                settings.devtools.width = size;
            } else {
                settings.devtools.height = size;
            }
        });
        cx.notify();
    }
}

/// The bar over `page`'s `DevTools` at `dock`: what they're of, where they can go, and a
/// button that closes them.
fn bar(page: &Entity<BrowserView>, title: &str, dock: Dock, p: Palette, cx: &App) -> Div {
    let place = |id: &'static str, glyph: IconName, label: &'static str, to: Dock| {
        let page = page.clone();
        icon_button(id, glyph, label, cx)
            .xsmall()
            .selected(dock == to)
            .on_click(move |_, _, cx| page.update(cx, |page, cx| page.dock_devtools(to, cx)))
    };
    let closing = page.clone();
    div()
        .h(px(30.))
        .flex_shrink_0()
        .w_full()
        .px_2()
        .flex()
        .items_center()
        .gap(px(2.))
        .border_b_1()
        .border_color(p.hairline)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_xs()
                .text_color(p.muted)
                .child(icon(IconName::Inspector, 13.))
                .child(ellipsized(format!("DevTools · {title}"))),
        )
        .child(place(
            "devtools-dock-left",
            IconName::PanelLeft,
            "Dock to the left",
            Dock::Left,
        ))
        .child(place(
            "devtools-dock-bottom",
            IconName::PanelBottom,
            "Dock to the bottom",
            Dock::Bottom,
        ))
        .child(place(
            "devtools-dock-right",
            IconName::PanelRight,
            "Dock to the right",
            Dock::Right,
        ))
        .child(place(
            "devtools-detach",
            IconName::SquareArrowOutUpRight,
            "Open in a pane of its own",
            Dock::Detached,
        ))
        .child(
            icon_button("devtools-close", IconName::X, "Close DevTools", cx)
                .xsmall()
                .on_click(move |_, _, cx| closing.update(cx, BrowserView::close_devtools)),
        )
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.settle(window, cx);
        self.make_devtools(window, cx);
        let page = self.page_element(window, cx);
        let column = input::steer(div().size_full().flex().flex_col(), cx);
        if let Some(inspected) = self.inspected().cloned() {
            // Detached, they carry their bar themselves.
            let of = inspected.read(cx);
            let detached = of.devtools_dock() == Some(Dock::Detached);
            let title = of.title();
            let p = theme::palette(cx);
            return column
                .when(detached, |column| {
                    column.child(bar(&inspected, &title, Dock::Detached, p, cx))
                })
                .child(div().flex_1().min_h_0().w_full().child(page));
        }
        let toolbar = toolbar::toolbar(self, window, cx);
        let body = self.body(page, cx);
        column
            .on_action(cx.listener(|this, _: &FindInPage, window, cx| this.open_find(window, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.find_again(true, cx)))
            .on_action(cx.listener(|this, _: &FindPrevious, _, cx| this.find_again(false, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_by(1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_by(-1, cx)))
            .on_action(cx.listener(|this, _: &ActualSize, _, cx| this.set_zoom(ACTUAL_SIZE, cx)))
            // The mouse's side buttons anywhere over the pane step through the page's
            // history, not the app's; heard on the way down, before the address field or a
            // button takes the press for itself.
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if let MouseButton::Navigate(direction) = event.button {
                    this.go(input::history_step(direction), cx);
                    cx.stop_propagation();
                }
            }))
            .child(toolbar)
            .child(body)
    }
}

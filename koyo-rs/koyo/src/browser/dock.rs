//! A page's `DevTools`: in the page's pane at its bottom, left or right, past a divider the
//! person drags, or detached into a pane of their own beside it in the tab. Where the last
//! ones went is where the next ones open, and the docked size holds for every page. They're
//! moved and closed from their own controls, whose asks the host passes on
//! (`docs/browser.md` 13).

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, AppContext as _, Context, Div, DragMoveEvent, Entity,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseUpEvent,
    ParentElement as _, Render, Stateful, Styled, Subscription, Window, deferred, div, px,
};
use koyo_browser_ipc::{self as ipc, DevToolsAsk, DevToolsDock};
use serde::{Deserialize, Serialize};

use super::find::ACTUAL_SIZE;
use super::view::{BrowserEvent, BrowserView};
use super::{ActualSize, FindInPage, FindNext, FindPrevious, ZoomIn, ZoomOut};
use super::{input, toolbar};
use crate::keyboard::{Cause, Intent, Surface};
use crate::settings;
use crate::theme;

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

impl From<DevToolsDock> for Dock {
    /// Undocked, the frontend's tools are in a pane of their own.
    fn from(side: DevToolsDock) -> Self {
        match side {
            DevToolsDock::Bottom => Dock::Bottom,
            DevToolsDock::Left => Dock::Left,
            DevToolsDock::Right => Dock::Right,
            DevToolsDock::Undocked => Dock::Detached,
        }
    }
}

impl From<Dock> for DevToolsDock {
    fn from(dock: Dock) -> Self {
        match dock {
            Dock::Bottom => DevToolsDock::Bottom,
            Dock::Left => DevToolsDock::Left,
            Dock::Right => DevToolsDock::Right,
            Dock::Detached => DevToolsDock::Undocked,
        }
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
    /// ⌥⌘I: the page's `DevTools` show, or hide. Hidden with the keyboard in them, the
    /// keyboard goes back to the page they inspect, as the person asked them gone; shown, it
    /// stays where it is, and `DevTools` asking for it as they open are refused.
    pub(super) fn toggle_devtools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let in_tools = |tools: &BrowserView| tools.focus.contains_focused(window, cx);
        let typing = match (self.inspected(), &self.devtools) {
            (Some(_), _) => in_tools(self),
            (None, Some(tools)) => in_tools(tools.view.read(cx)),
            (None, None) => false,
        };
        if typing {
            let (focus, pane) = match self.inspected() {
                Some(page) => (page.read(cx).focus.clone(), page.read(cx).pane),
                None => (self.focus.clone(), self.pane),
            };
            crate::keyboard::set(window, Surface::Page(pane), Cause::Intent(Intent::DevTools));
            window.focus(&focus, cx);
        }
        self.inspect(None, cx);
    }

    /// Shows the page's `DevTools`, revealing the element at `at` when there is one; asked
    /// without one while they show, hides them. Asked of the `DevTools` themselves, as with
    /// the keys in them, it's their page that hides them, once this update is done.
    pub fn inspect(&mut self, at: Option<ipc::Point>, cx: &mut Context<Self>) {
        if let Some(page) = self.inspected().cloned() {
            if at.is_none() {
                cx.defer(move |cx| page.update(cx, BrowserView::close_devtools));
            }
            return;
        }
        match (&self.devtools, at) {
            (Some(tools), Some(at)) => {
                let (page, side) = (self.pane, self.devtools_side(cx));
                tools.view.update(cx, |tools, cx| tools.reveal(at, page, side, cx));
            }
            (Some(_), None) => self.close_devtools(cx),
            (None, at) => {
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

    /// The side the page's `DevTools` frontend opens at: where they are, else where the
    /// next ones open.
    pub(super) fn devtools_side(&self, cx: &App) -> DevToolsDock {
        (self.devtools_dock())
            .unwrap_or(settings::get(cx).devtools.dock)
            .into()
    }

    /// What the page's `DevTools` asked from their own controls (`docs/browser.md` 13.2): to
    /// be at a side, undocked into a pane of their own, closed, or the page in responsive mode
    /// or out of it. A side they're at already, or a mode the page is in, changes nothing.
    pub(super) fn devtools_asked(&mut self, ask: DevToolsAsk, cx: &mut Context<Self>) {
        match ask {
            DevToolsAsk::Dock { side } => {
                let dock = Dock::from(side);
                if self.devtools_dock().is_some_and(|open| open != dock) {
                    self.dock_devtools(dock, cx);
                }
            }
            DevToolsAsk::Close => self.close_devtools(cx),
            DevToolsAsk::DeviceMode { on } => {
                if self.devtools.is_some() && self.responsive.mode.is_some() != on {
                    self.toggle_responsive(cx);
                }
            }
        }
    }

    /// The page's open `DevTools`.
    pub fn devtools(&self) -> Option<&Entity<BrowserView>> {
        self.devtools.as_ref().map(|tools| &tools.view)
    }

    /// The page under the toolbar, under the device bar in responsive mode, with its
    /// `DevTools` beside it while they're docked.
    fn body(&mut self, page: Stateful<Div>, device: Option<Div>, cx: &mut Context<Self>) -> Div {
        let body = div().flex_1().min_h_0().min_w_0().w_full().flex();
        let page = div()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .children(device)
            .child(div().flex_1().min_h_0().w_full().child(page))
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
            .child(div().flex_1().min_h_0().min_w_0().child(tools));
        let id = self.pane;
        let divider = theme::card::divider(
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
                    let (span, reach) = match dock {
                        Dock::Bottom => (bounds.size.height, bounds.bottom() - at.y),
                        Dock::Right => (bounds.size.width, bounds.right() - at.x),
                        Dock::Left => (bounds.size.width, at.x - bounds.left()),
                        Dock::Detached => return,
                    };
                    let band = theme::card::DIVIDER;
                    let room = (span.as_f32() - band - DOCK_MIN).floor().max(DOCK_MIN);
                    let size = (reach.as_f32() - band / 2.).round().clamp(DOCK_MIN, room);
                    if this.dock_drag != Some(size) {
                        this.dock_drag = Some(size);
                        cx.notify();
                    }
                }),
            )
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

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.settle(window, cx);
        self.make_devtools(window, cx);
        let page = self.page_element(window, cx);
        let column = input::steer(div().size_full().flex().flex_col(), cx);
        if self.fills_screen() || self.is_inspector() {
            return column.child(div().flex_1().min_h_0().w_full().child(page));
        }
        let arc = settings::get(cx).layout == settings::Layout::Arc;
        let toolbar = (!arc).then(|| toolbar::toolbar(self, window, cx));
        let device = super::responsive::bar::device_bar(self, window, cx);
        let body = self.body(page, device, cx);
        column
            .on_action(cx.listener(|this, _: &FindInPage, window, cx| this.open_find(window, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.find_again(true, cx)))
            .on_action(cx.listener(|this, _: &FindPrevious, _, cx| this.find_again(false, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_by(1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_by(-1, cx)))
            .on_action(cx.listener(|this, _: &ActualSize, _, cx| this.set_zoom(ACTUAL_SIZE, cx)))
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if let MouseButton::Navigate(direction) = event.button {
                    this.go(input::history_step(direction), cx);
                    cx.stop_propagation();
                }
            }))
            .children(toolbar)
            .child(body)
    }
}

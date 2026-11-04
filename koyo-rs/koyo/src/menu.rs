//! Dropdown menus in a popup window of their own, so the system blurs what lies behind them
//! like the window's glass: GPUI can only blur behind a whole window, never behind an element.
//! Each row dispatches an action to a focus handle in the window that opened the menu. The
//! menu closes itself on a pick, Escape, a press outside it, or its window losing focus.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyWindowHandle, App, AppContext as _, Bounds, Context, ElementId, FocusHandle, Global,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Render, Rems,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};

use crate::shell::icon;
use crate::theme;

/// The narrowest a menu gets, and the widest: past that, a row's label wraps. A label can be what a
/// person agrees to, so it's never cut.
const WIDTH: f32 = 220.;
const MAX_WIDTH: f32 = 420.;
const PADDING: f32 = 5.;
/// How tall a line of a label is; what a row adds past its first.
const LABEL_LINE: f32 = 18.;
const ROW_PADDING: f32 = 8.;
const ICON: f32 = 15.;
const ROW_GAP: f32 = 10.;
const LABEL_SIZE: Rems = Rems(0.875);
/// What a row holds beside its label: its padding, its icon and the gap after it.
const ROW_ROOM: f32 = ROW_PADDING * 2. + ICON + ROW_GAP;
/// What a shortcut takes of a row, and the gap before it: a row's label is measured for what's left.
const SHORTCUT: f32 = 50.;
const SHORTCUT_ROOM: f32 = SHORTCUT + ROW_GAP;
/// How long the parent window may be inactive before the menu takes it as left, in ms:
/// long enough for the menu's own activation to arrive after a click on a row.
const ACTIVATION_GRACE: u64 = 80;
/// Space between the button and the menu.
const GAP: f32 = 4.;

pub struct MenuItem {
    pub icon: IconName,
    pub label: SharedString,
    pub action: Box<dyn Action>,
}

/// The menu on screen, if any, and the button that opened it.
#[derive(Default)]
struct OpenMenu(Option<(ElementId, AnyWindowHandle)>);

impl Global for OpenMenu {}

/// Wraps `trigger` so pressing it toggles a menu of `items` below it, or over it with no room
/// below. `items` is only called when the menu opens.
pub fn dropdown(
    id: impl Into<ElementId>,
    trigger: impl IntoElement,
    target: FocusHandle,
    items: impl Fn() -> Vec<MenuItem> + 'static,
) -> impl IntoElement {
    let id = id.into();
    let outside = id.clone();
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let measured = bounds.clone();
    div()
        .relative()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            let reopening = is_open(&id, cx);
            close(cx);
            if !reopening {
                open(id.clone(), bounds.get(), items(), &target, window, cx);
            }
        })
        .on_mouse_down_out(move |_, _, cx| {
            if is_open(&outside, cx) {
                close(cx);
            }
        })
        .child(trigger)
        .on_prepaint(move |bounds, _, _| measured.set(bounds))
}

/// Where the middle of row `index` is in the menu's window, for tests that click one.
#[cfg(test)]
pub(crate) fn row_center(index: usize) -> gpui_kit::Point<Pixels> {
    point(
        px(WIDTH / 2.),
        px(PADDING + theme::ROW_HEIGHT * (index as f32 + 0.5)),
    )
}

/// Whether the open menu's own window is the active one now.
fn menu_is_active(cx: &mut App) -> bool {
    let Some(menu) = cx
        .try_global::<OpenMenu>()
        .and_then(|open| open.0.as_ref())
        .map(|(_, menu)| *menu)
    else {
        return false;
    };
    menu.update(cx, |_, window, _| window.is_window_active())
        .unwrap_or(false)
}

fn is_open(id: &ElementId, cx: &App) -> bool {
    cx.try_global::<OpenMenu>()
        .and_then(|open| open.0.as_ref())
        .is_some_and(|(open, _)| open == id)
}

/// Closes the open menu, if any. It can't reach the menu's own window from inside it.
fn close(cx: &mut App) {
    let Some((_, window)) = cx.default_global::<OpenMenu>().0.take() else {
        return;
    };
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

pub(crate) fn open(
    id: ElementId,
    anchor: Bounds<Pixels>,
    items: Vec<MenuItem>,
    target: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    // Shortcuts are looked up here, where the target's key bindings are in scope.
    let items: Vec<_> = items
        .into_iter()
        .map(|item| {
            let shortcut = Kbd::binding_for_action_in(&*item.action, target, window);
            (item, shortcut)
        })
        .collect();
    let (width, height) = fitted(&items, window);
    // Right of the window's edge it'd lie over what isn't the app's, so it opens leftwards.
    let left = if anchor.left() + width > window.bounds().size.width {
        anchor.right() - width
    } else {
        anchor.left()
    };
    // Past its bottom edge, likewise, it opens upwards.
    let top = if anchor.bottom() + px(GAP) + height > window.bounds().size.height {
        anchor.top() - px(GAP) - height
    } else {
        anchor.bottom() + px(GAP)
    };
    let origin = window.bounds().origin + point(left, top);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            origin,
            size(width, height),
        ))),
        titlebar: None,
        focus: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Blurred,
        display_id: window.display(cx).map(|display| display.id()),
        ..Default::default()
    };
    let parent = window.window_handle();
    let target = target.clone();
    let menu = cx.new(|cx| Menu {
        _subscriptions: vec![
            // The parent window keeps focus while the menu is up, so it hears Escape and
            // knows when the user leaves for another window or app. A click on a row makes
            // the menu's own window the active one instead, a moment after the parent loses
            // it, so the menu waits for that before taking the loss as leaving.
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    return;
                }
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(ACTIVATION_GRACE))
                        .await;
                    cx.update(|cx| {
                        if !menu_is_active(cx) {
                            close(cx);
                        }
                    });
                })
                .detach();
            }),
            cx.intercept_keystrokes(|event, _, cx| {
                if event.keystroke.key == "escape" {
                    close(cx);
                }
            }),
        ],
        items,
        target,
        parent,
    });
    let Ok(handle) = cx.open_window(options, |_, _| menu.clone()) else {
        return;
    };
    cx.set_global(OpenMenu(Some((id, handle.into()))));
    // Once the menu's own window is the active one, losing that is leaving too.
    handle
        .update(cx, |menu, window, cx| {
            let left = cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    close(cx);
                }
            });
            menu._subscriptions.push(left);
        })
        .ok();
}

/// How wide a menu is, for its longest row within what a menu can be, and how tall its rows then
/// are, those whose labels had to wrap taking more.
fn fitted(items: &[(MenuItem, Option<Kbd>)], window: &Window) -> (Pixels, Pixels) {
    let style = window.text_style();
    let font_size = LABEL_SIZE.to_pixels(window.rem_size());
    let run = |label: &SharedString| style.to_run(label.len());
    let beside = |shortcut: &Option<Kbd>| {
        px(ROW_ROOM + if shortcut.is_some() { SHORTCUT_ROOM } else { 0. })
    };
    let longest = items.iter().map(|(item, shortcut)| {
        let label = &item.label;
        let line = window.text_system().shape_line(label.clone(), font_size, &[run(label)], None);
        line.width + beside(shortcut)
    });
    // A pixel over, for a label to fit in what it measures rather than wrap at its very end.
    let width = longest.fold(px(0.), Pixels::max) + px(PADDING * 2. + 1.);
    let width = width.clamp(px(WIDTH), px(MAX_WIDTH));
    let rows = items.iter().map(|(item, shortcut)| {
        let label = &item.label;
        let room = width - px(PADDING * 2.) - beside(shortcut);
        let lines = (window.text_system())
            .shape_text(label.clone(), font_size, &[run(label)], Some(room), None)
            .map_or(1, |shaped| shaped.iter().map(|line| line.wrap_boundaries.len() + 1).sum());
        theme::ROW_HEIGHT + LABEL_LINE * (lines - 1) as f32
    });
    (width, px(PADDING * 2. + rows.sum::<f32>()))
}

struct Menu {
    items: Vec<(MenuItem, Option<Kbd>)>,
    target: FocusHandle,
    parent: AnyWindowHandle,
    _subscriptions: Vec<Subscription>,
}

impl Menu {
    fn pick(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.set_global(OpenMenu(None));
        window.remove_window();
        let (item, _) = &self.items[index];
        self.parent
            .update(cx, |_, window, cx| {
                self.target.dispatch_action(&*item.action, window, cx);
            })
            .ok();
    }
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .size_full()
            .p(px(PADDING))
            .flex()
            .flex_col()
            .bg(p.menu)
            .text_color(p.text)
            .children(
                self.items
                    .iter()
                    .enumerate()
                    .map(|(index, (item, shortcut))| {
                        div()
                            .id(index)
                            .min_h(px(theme::ROW_HEIGHT))
                            .flex_shrink_0()
                            .px(px(ROW_PADDING))
                            .flex()
                            .items_center()
                            .gap(px(ROW_GAP))
                            .rounded(px(6.))
                            .text_size(LABEL_SIZE)
                            .cursor_pointer()
                            .hover(|style| style.bg(p.selected))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.pick(index, window, cx);
                            }))
                            .child(icon(item.icon, ICON).text_color(p.muted))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .line_height(px(LABEL_LINE))
                                    .child(item.label.clone()),
                            )
                            .when_some(shortcut.clone(), |row, keys| {
                                row.child(
                                    div()
                                        .flex()
                                        .flex_shrink_0()
                                        .justify_end()
                                        .w(px(SHORTCUT))
                                        .text_xs()
                                        .text_color(p.muted)
                                        .child(keys.appearance(false)),
                                )
                            })
                    }),
            )
    }
}

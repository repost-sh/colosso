//! Dropdown menus in a popup window of their own, so the system blurs what lies behind them
//! like the window's glass: GPUI can only blur behind a whole window, never behind an element.
//! Each row dispatches an action to a focus handle in the window that opened the menu. The
//! menu closes itself on a pick, Escape, a press outside it, or its window losing focus.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyWindowHandle, App, AppContext as _, Bounds, Context, ElementId, FocusHandle, Global,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};

use crate::shell::icon;
use crate::theme;

const WIDTH: f32 = 220.;
const PADDING: f32 = 5.;
/// Space between the button and the menu below it.
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

/// Wraps `trigger` so pressing it toggles a menu of `items` below it. `items` is only
/// called when the menu opens.
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

fn open(
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
    let origin = window.bounds().origin + point(anchor.left(), anchor.bottom() + px(GAP));
    let height = PADDING * 2. + theme::ROW_HEIGHT * items.len() as f32;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            origin,
            size(px(WIDTH), px(height)),
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
        _subscriptions: [
            // The parent window keeps focus while the menu is up, so it hears Escape and
            // knows when the user leaves for another window or app.
            cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    close(cx);
                }
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
    if let Ok(handle) = cx.open_window(options, |_, _| menu) {
        cx.set_global(OpenMenu(Some((id, handle.into()))));
    }
}

struct Menu {
    items: Vec<(MenuItem, Option<Kbd>)>,
    target: FocusHandle,
    parent: AnyWindowHandle,
    _subscriptions: [Subscription; 2],
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
                            .h(px(theme::ROW_HEIGHT))
                            .flex_shrink_0()
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded(px(6.))
                            .text_sm()
                            .cursor_pointer()
                            .hover(|style| style.bg(p.selected))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.pick(index, window, cx);
                            }))
                            .child(icon(item.icon, 15.).text_color(p.muted))
                            .child(div().flex_1().child(item.label.clone()))
                            .when_some(shortcut.clone(), |row, keys| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(p.muted)
                                        .child(keys.appearance(false)),
                                )
                            })
                    }),
            )
    }
}

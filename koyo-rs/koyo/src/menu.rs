//! Dropdown menus in a popup window of their own, so the system blurs what lies behind them
//! like the window's glass: GPUI can only blur behind a whole window, never behind an element.
//! The window holds a body: rows, each dispatching an action to a focus handle in the window that
//! opened the menu, or a panel, a view of its own. The menu closes itself on a pick, Escape, a press
//! outside it, or its window losing focus. That window keeps the keys while it's up, so what the
//! body has a use for is heard ahead of it.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::{
    Action, AnyWindowHandle, App, AppContext as _, Bounds, Context, ElementId, Entity, FocusHandle,
    Global, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _,
    Pixels, Rems, Render, SharedString, Size, Styled as _, Window, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};

use self::view::{Menu, Rows};
use crate::theme;

mod switch;
mod view;

pub(crate) use view::row;

/// The narrowest a menu gets, and the widest: past that, a row's label wraps. A label can be what a
/// person agrees to, so it's never cut.
const WIDTH: f32 = 220.;
const MAX_WIDTH: f32 = 420.;
pub(crate) const PADDING: f32 = 5.;
/// How tall a line of a label is; what a row adds past its first.
const LABEL_LINE: f32 = 18.;
pub(crate) const ROW_PADDING: f32 = 8.;
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
/// What's clear on each side of the line that sets a switch's row apart, and all it takes of a menu.
const SEPARATOR_MARGIN: f32 = 4.;
const SEPARATOR: f32 = 1. + SEPARATOR_MARGIN * 2.;
/// How many rows the page keys move.
const PAGE: isize = 5;
/// Space between the button and the menu.
const GAP: f32 = 4.;

pub struct MenuItem {
    /// What marks it; with none, its label still starts where the others' do.
    pub icon: Option<IconName>,
    pub label: SharedString,
    pub action: Box<dyn Action>,
    /// Whether it's on, if it's a switch: a pick flips it, and leaves the menu up.
    pub switch: Option<bool>,
    /// Whether a line sets it apart from those above.
    pub apart: bool,
}

impl MenuItem {
    pub fn new(
        icon: Option<IconName>,
        label: impl Into<SharedString>,
        action: Box<dyn Action>,
    ) -> Self {
        MenuItem {
            icon,
            label: label.into(),
            action,
            switch: None,
            apart: false,
        }
    }

    /// A switch, on or off.
    pub fn switch(mut self, on: bool) -> Self {
        self.switch = Some(on);
        self
    }

    /// With a line above it, or not.
    pub fn apart(mut self, apart: bool) -> Self {
        self.apart = apart;
        self
    }
}

/// Whether a line sets row `index` apart from those above it.
fn separated(rows: &[(MenuItem, Option<Kbd>)], index: usize) -> bool {
    index > 0 && rows.get(index).is_some_and(|(item, _)| item.apart)
}

/// What a menu's window holds, which hears the keys it has a use for ahead of the window that has
/// them. Escape closes any menu, and so does Enter on a body with no use for it.
pub trait MenuBody: Render {
    fn hears(&mut self, key: &str, cx: &mut Context<Self>) -> bool;
}

/// The menu on screen, if any, and the button that opened it.
#[derive(Default)]
struct OpenMenu(Option<(ElementId, AnyWindowHandle)>);

impl Global for OpenMenu {}

/// What opens a menu, by a press or by a key: where it's drawn is where the menu goes either way,
/// below it, or over it with no room below.
#[derive(Clone)]
pub struct Trigger {
    id: ElementId,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Trigger {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Trigger {
            id: id.into(),
            bounds: Rc::default(),
        }
    }

    /// `trigger`, measured, whose press `pressed` hears. A press outside closes what this opened.
    pub fn wraps<E, P>(&self, trigger: E, pressed: P) -> impl IntoElement + use<E, P>
    where
        E: IntoElement,
        P: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    {
        let outside = self.id.clone();
        let measured = self.bounds.clone();
        div()
            .relative()
            .on_mouse_down(MouseButton::Left, pressed)
            .on_mouse_down_out(move |_, _, cx| {
                if is_open(&outside, cx) {
                    close(cx);
                }
            })
            .child(trigger)
            .on_prepaint(move |bounds, _, _| measured.set(bounds))
    }

    pub fn is_open(&self, cx: &App) -> bool {
        is_open(&self.id, cx)
    }

    /// Closes what this opened, if it's still up.
    pub fn close(&self, cx: &mut App) {
        if self.is_open(cx) {
            close(cx);
        }
    }

    /// Closes what's open, and unless that was this, opens a menu of `items`, `highlighted` the row
    /// the keys start from.
    pub fn toggle(
        &self,
        items: impl FnOnce(&App) -> Vec<MenuItem>,
        highlighted: Option<usize>,
        target: &FocusHandle,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.clears_the_way(cx) {
            let items = items(cx);
            open_rows(
                self.id.clone(),
                self.bounds.get(),
                items,
                highlighted,
                target,
                window,
                cx,
            );
        }
    }

    /// Closes what's open, and unless that was this, opens `panel`, in a menu's window this big.
    pub fn toggle_panel<B: MenuBody>(
        &self,
        extent: Size<Pixels>,
        panel: impl FnOnce(&mut App) -> Entity<B>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.clears_the_way(cx) {
            let panel = panel(cx);
            open_surface(
                self.id.clone(),
                self.bounds.get(),
                extent,
                panel,
                window,
                cx,
            );
        }
    }

    /// Closes what's open, and says whether this is to open: not if it was what's open.
    fn clears_the_way(&self, cx: &mut App) -> bool {
        let reopening = self.is_open(cx);
        close(cx);
        !reopening
    }
}

/// Wraps `trigger` so pressing it toggles a menu of `items` below it, or over it with no room
/// below. `items` is only called when the menu opens.
pub fn dropdown(
    id: impl Into<ElementId>,
    trigger: impl IntoElement,
    target: FocusHandle,
    items: impl Fn(&App) -> Vec<MenuItem> + 'static,
) -> impl IntoElement {
    let opener = Trigger::new(id);
    let toggled = opener.clone();
    opener.wraps(trigger, move |_, window, cx| {
        toggled.toggle(&items, None, &target, window, cx);
    })
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

/// Closes the open menu, if any. Its window goes once nothing is in it: what asks may be in that very
/// window, as when it hears it's no longer the active one, and a window can't be reached from inside.
fn close(cx: &mut App) {
    let Some((_, window)) = cx.default_global::<OpenMenu>().0.take() else {
        return;
    };
    cx.defer(move |cx| {
        window
            .update(cx, |_, window, _| window.remove_window())
            .ok();
    });
}

pub(crate) fn open_rows(
    id: ElementId,
    anchor: Bounds<Pixels>,
    items: Vec<MenuItem>,
    highlighted: Option<usize>,
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
    let extent = fitted(&items, window);
    let target = target.clone();
    let parent = window.window_handle();
    let rows = cx.new(|_| Rows {
        items,
        highlighted,
        target,
        parent,
    });
    open_surface(id, anchor, extent, rows, window, cx);
}

/// The menu's window, this big, holding `body`.
fn open_surface<B: MenuBody>(
    id: ElementId,
    anchor: Bounds<Pixels>,
    extent: Size<Pixels>,
    body: Entity<B>,
    window: &mut Window,
    cx: &mut App,
) {
    // Right of the window's edge it'd lie over what isn't the app's, so it opens leftwards.
    let left = if anchor.left() + extent.width > window.bounds().size.width {
        anchor.right() - extent.width
    } else {
        anchor.left()
    };
    // Past its bottom edge, likewise, it opens upwards.
    let top = if anchor.bottom() + px(GAP) + extent.height > window.bounds().size.height {
        anchor.top() - px(GAP) - extent.height
    } else {
        anchor.bottom() + px(GAP)
    };
    let origin = window.bounds().origin + point(left, top);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, extent))),
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
    let menu = cx.new(|cx: &mut Context<Menu>| {
        let hearing = body.downgrade();
        Menu {
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
                cx.intercept_keystrokes(move |event, _, cx| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    let key = event.keystroke.key.as_str();
                    let heard = hearing
                        .update(cx, |body, cx| body.hears(key, cx))
                        .unwrap_or(false);
                    let closing = !heard && matches!(key, "escape" | "enter");
                    if closing {
                        close(cx);
                    }
                    if heard || closing {
                        cx.stop_propagation();
                    }
                }),
            ],
            body: body.into(),
        }
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
fn fitted(items: &[(MenuItem, Option<Kbd>)], window: &Window) -> Size<Pixels> {
    let style = window.text_style();
    let font_size = LABEL_SIZE.to_pixels(window.rem_size());
    let run = |label: &SharedString| style.to_run(label.len());
    let beside = |(item, shortcut): &(MenuItem, Option<Kbd>)| {
        let trailing = shortcut.is_some() || item.switch.is_some();
        px(ROW_ROOM + if trailing { SHORTCUT_ROOM } else { 0. })
    };
    let longest = items.iter().map(|row| {
        let label = &row.0.label;
        let line = window
            .text_system()
            .shape_line(label.clone(), font_size, &[run(label)], None);
        line.width + beside(row)
    });
    // A pixel over, for a label to fit in what it measures rather than wrap at its very end.
    let width = longest.fold(px(0.), Pixels::max) + px(PADDING * 2. + 1.);
    let width = width.clamp(px(WIDTH), px(MAX_WIDTH));
    let rows = items.iter().enumerate().map(|(index, row)| {
        let label = &row.0.label;
        let room = width - px(PADDING * 2.) - beside(row);
        let lines = (window.text_system())
            .shape_text(label.clone(), font_size, &[run(label)], Some(room), None)
            .map_or(1, |shaped| {
                shaped
                    .iter()
                    .map(|line| line.wrap_boundaries.len() + 1)
                    .sum()
            });
        let above = if separated(items, index) {
            SEPARATOR
        } else {
            0.
        };
        above + theme::ROW_HEIGHT + LABEL_LINE * (lines - 1) as f32
    });
    size(width, px(PADDING * 2. + rows.sum::<f32>()))
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, WindowHandle};

    use super::*;

    struct Opener(FocusHandle);

    impl Render for Opener {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// A window with a menu open, and the menu's own window.
    fn opened(cx: &mut TestAppContext) -> (WindowHandle<Opener>, AnyWindowHandle) {
        cx.update(crate::init);
        let opener = cx.add_window(|_, cx| Opener(cx.focus_handle()));
        opener
            .update(cx, |opener, window, cx| {
                let target = opener.0.clone();
                open_rows(
                    "test-menu".into(),
                    Bounds::default(),
                    Vec::new(),
                    None,
                    &target,
                    window,
                    cx,
                );
            })
            .unwrap();
        cx.run_until_parked();
        let parent: AnyWindowHandle = opener.into();
        let menu = cx
            .windows()
            .into_iter()
            .find(|window| *window != parent)
            .unwrap();
        (opener, menu)
    }

    #[gpui_kit::test]
    fn a_menu_closed_from_inside_its_own_window_goes(cx: &mut TestAppContext) {
        let (_, menu) = opened(cx);

        // As when it's lost being the active window, after a press in it that leaves it up.
        menu.update(cx, |_, _, cx| close(cx)).unwrap();
        cx.run_until_parked();

        assert!(
            !cx.windows().contains(&menu),
            "nothing else could ever close it"
        );
    }
}

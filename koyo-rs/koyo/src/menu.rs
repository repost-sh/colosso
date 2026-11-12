//! Dropdown menus in a popup window of their own, so the system blurs what lies behind them
//! like the window's glass: GPUI can only blur behind a whole window, never behind an element.
//! The window holds a body: rows, each dispatching an action to a focus handle in the window that
//! opened the menu, or a panel, a view of its own. The menu closes itself on a pick, and on a key
//! it has no use for, which then goes on to the window behind; and as every floating window goes
//! (`popup.rs`): on a press anywhere else in that window, its page changing, or the person
//! leaving it. That window keeps the keys while it's up, so what the body has a use for is heard
//! ahead of it. A panel fits what it holds as that changes, keeping the edge it opened from.

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::{
    Action, App, AppContext as _, Bounds, Context, ElementId, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Rems, Render, SharedString, Size, Styled as _, Window, div, point, px, size,
};

use self::view::{Menu, Rows};
use crate::popup::{self, Kind, Opening};
use crate::theme;
use crate::wrapped::wrapped_lines;

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
/// them; any other key closes the menu, Escape and Enter only that. The window pads it by
/// [`PADDING`] all round: a body adds none of its own, and its extent is its content's.
pub trait MenuBody: Render {
    fn hears(&mut self, key: &str, cx: &mut Context<Self>) -> bool;

    /// How big its window is, drawn in `window`: asked as it opens, and again each time it
    /// changes, as the window fits it.
    fn extent(&self, window: &Window, cx: &App) -> Size<Pixels>;
}

/// What opens a menu, by a press or by a key: where it's drawn is where the menu goes either way,
/// below it, or over it with no room below.
#[derive(Clone)]
pub struct Trigger(popup::Source);

impl Trigger {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Trigger(popup::Source::new(id))
    }

    /// `trigger`, measured, whose press `pressed` hears. A press anywhere else in the window
    /// closes what this opened (`popup::pressed`).
    pub fn wraps<E, P>(&self, trigger: E, pressed: P) -> impl IntoElement + use<E, P>
    where
        E: IntoElement,
        P: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    {
        let measured = self.0.drawn.clone();
        div()
            .relative()
            .on_mouse_down(MouseButton::Left, pressed)
            .child(trigger)
            .on_prepaint(move |bounds, _, _| measured.set(bounds))
    }

    pub fn is_open(&self, cx: &App) -> bool {
        popup::opened_from(Kind::Menu, cx).as_ref() == Some(&self.0.id)
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
            open_rows(self.0.clone(), items, highlighted, target, window, cx);
        }
    }

    /// Closes what's open, and unless that was this, opens `panel` in a menu's window.
    pub fn toggle_panel<B: MenuBody>(
        &self,
        panel: impl FnOnce(&mut App) -> Entity<B>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.clears_the_way(cx) {
            let panel = panel(cx);
            open_surface(self.0.clone(), panel, window, cx);
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

/// Closes the open menu, if any.
fn close(cx: &mut App) {
    popup::close(Kind::Menu, cx);
}

/// Opens a menu of `items` from `source`, `highlighted` the row the keys start from.
pub(crate) fn open_rows(
    source: popup::Source,
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
    let target = target.clone();
    let parent = window.window_handle();
    let extent = fitted(&items, window);
    let rows = cx.new(|_| Rows {
        items,
        extent,
        highlighted,
        target,
        parent,
    });
    open_surface(source, rows, window, cx);
}

/// The menu's window holding `body`, by where `source` was last drawn.
fn open_surface<B: MenuBody>(
    source: popup::Source,
    body: Entity<B>,
    window: &mut Window,
    cx: &mut App,
) {
    let anchor = source.drawn.get();
    let extent = padded(body.read(cx).extent(window, cx));
    // Right of the window's edge it'd lie over what isn't the app's, so it opens leftwards.
    let leftward = anchor.left() + extent.width > window.bounds().size.width;
    let left = if leftward {
        anchor.right() - extent.width
    } else {
        anchor.left()
    };
    // Past its bottom edge, likewise, it opens upwards.
    let upward = anchor.bottom() + px(GAP) + extent.height > window.bounds().size.height;
    let top = if upward {
        anchor.top() - px(GAP) - extent.height
    } else {
        anchor.bottom() + px(GAP)
    };
    // It keeps the corner nearest the trigger as it fits what it holds.
    let corner = popup::Corner {
        top: !upward,
        left: !leftward,
    };
    let origin = window.bounds().origin + point(left, top);
    let hearing = body.downgrade();
    let opening = Opening {
        kind: Kind::Menu,
        bounds: Bounds::new(origin, extent),
        kept: false,
        source: Some(source),
    };
    popup::open(opening, window, cx, |menu, cx: &mut Context<Menu>| Menu {
        // The window behind keeps focus while the menu is up, so it hears the keys: the menu's
        // own; Escape and Enter, which only close it; and any other, which goes on to the window
        // behind and then closes the menu, as a native one's does. Only then, so a shortcut that
        // toggles the menu finds it up, and closes it.
        _keys: cx.intercept_keystrokes(move |event, _, cx| {
            let keystroke = &event.keystroke;
            let key = keystroke.key.as_str();
            let plain = !keystroke.modifiers.modified();
            let heard = plain
                && hearing
                    .update(cx, |body, cx| body.hears(key, cx))
                    .unwrap_or(false);
            if heard {
                cx.stop_propagation();
            } else if plain && matches!(key, "escape" | "enter") {
                close(cx);
                cx.stop_propagation();
            } else {
                let up = popup::window(Kind::Menu, cx);
                cx.defer(move |cx| {
                    if up.is_some() && popup::window(Kind::Menu, cx) == up {
                        close(cx);
                    }
                });
            }
        }),
        // A panel whose content changes fits it.
        _fits: cx.observe_in(&body, menu, move |_, body, window, cx| {
            let extent = padded(body.read(cx).extent(window, cx));
            popup::fit(window, extent, corner, cx);
        }),
        body: body.into(),
    });
}

/// A body's `extent` with the window's padding round it.
fn padded(extent: Size<Pixels>) -> Size<Pixels> {
    size(
        extent.width + px(2. * PADDING),
        extent.height + px(2. * PADDING),
    )
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
    let width = longest.fold(px(0.), Pixels::max) + px(1.);
    let width = width.clamp(px(WIDTH - 2. * PADDING), px(MAX_WIDTH - 2. * PADDING));
    let rows = items.iter().enumerate().map(|(index, row)| {
        let label = &row.0.label;
        let room = width - beside(row);
        let lines = wrapped_lines(label, &style, font_size, room, window);
        let above = if separated(items, index) {
            SEPARATOR
        } else {
            0.
        };
        above + theme::ROW_HEIGHT + LABEL_LINE * (lines - 1) as f32
    });
    size(width, px(rows.sum::<f32>()))
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AnyWindowHandle, TestAppContext, WindowHandle};

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
                    popup::Source::new("test-menu"),
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

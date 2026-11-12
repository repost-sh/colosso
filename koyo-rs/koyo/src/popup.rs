//! The app's floating windows: menus and their panels (`menu.rs`), dialogs (`modal.rs`), the
//! theme picker (`picker.rs`) and the sidebar's hover cards (`arc/card.rs`). Each is a glass
//! window over the window it belongs to, opened here, one of each kind up at a time, and they all
//! float and go the same way:
//!
//! - It stays on the Space it opened on, and goes unseen while Koyo isn't the app in front. GPUI
//!   has a pop-up panel join every Space, so one left open went with the person to another
//!   desktop and stayed over whatever they did there.
//! - It goes on a press anywhere in the window it floats over, heard on the way down by a
//!   [`listener`] drawn in that window, whatever is over the spot; but not on the trigger it was
//!   opened from, which toggles it itself, whether or not the page still draws that trigger.
//! - It goes as that window comes to show something else ([`showing`]): another page, layout or
//!   tab, the sidebar folded; and as that window moves or is resized, which would leave it
//!   hanging away from what it belongs to. A dialog stays as the window changes what it shows:
//!   it's a task of its own.
//! - Unless it's a kept dialog, it goes once neither it nor the window it belongs to is the one in
//!   front: the person went to another window, another app or another Space.
//! - Its edge is worked out again once its first frames are in, and a panel can fit what it holds
//!   as that changes, keeping the corner it opened from ([`fit`]).

use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher as _};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, DispatchPhase, ElementId, Global,
    IntoElement, MouseDownEvent, Pixels, Point, Render, Size, Styled as _, Subscription, Task,
    Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    canvas,
};

/// How long after a window's activation changes the leaving rule looks, in ms: the window gaining
/// it hears so only after the one losing it, as when a click on a menu's row moves it from the
/// window behind to the menu.
const SETTLE: u64 = 80;

/// The kinds of floating window: one of each is up at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Menu,
    Dialog,
    Picker,
    Card,
}

impl Kind {
    /// A dialog takes the keyboard, and stays as the window under it shows something else: it's a
    /// task of its own.
    fn is_task(self) -> bool {
        self == Kind::Dialog
    }
}

/// What a floating window was opened from, a menu's trigger: an id, and where it was last drawn,
/// in the window it's in; a press there is the trigger's own.
#[derive(Clone)]
pub struct Source {
    pub id: ElementId,
    pub drawn: Rc<Cell<Bounds<Pixels>>>,
}

impl Source {
    /// A trigger called `id`, not drawn yet.
    pub fn new(id: impl Into<ElementId>) -> Source {
        Source {
            id: id.into(),
            drawn: Rc::default(),
        }
    }
}

/// A floating window to open.
pub struct Opening {
    pub kind: Kind,
    /// Where, on the screen.
    pub bounds: Bounds<Pixels>,
    /// Whether it stays while the person is away, a dialog they leave Koyo for and come back to:
    /// only unseen meanwhile.
    pub kept: bool,
    pub source: Option<Source>,
}

/// A floating window that's up.
struct Up {
    window: AnyWindowHandle,
    /// The window it floats over.
    over: AnyWindowHandle,
    source: Option<Source>,
    /// Its watch on its own window and the window under it: held here, so they go as it does.
    _watching: Vec<Subscription>,
    /// The look, once activation settles, at whether the person has left it: the latest
    /// replaces the one before.
    settling: Option<Task<()>>,
}

#[derive(Default)]
struct Popups {
    up: HashMap<Kind, Up>,
    /// What each window floating windows open over showed last, hashed.
    showing: HashMap<AnyWindowHandle, u64>,
}

impl Global for Popups {}

/// What a floating window over `parent` opens with, at `bounds` on its screen: no title bar, not
/// moved or resized, and taking the keyboard if it should `focus`.
fn options(bounds: Bounds<Pixels>, focus: bool, parent: &Window, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Blurred,
        display_id: parent.display(cx).map(|display| display.id()),
        ..Default::default()
    }
}

/// Opens the floating window `build` makes over `parent`, as `opening` says, after closing the one
/// of its kind that's up.
pub fn open<V: Render>(
    opening: Opening,
    parent: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> Option<WindowHandle<V>> {
    let Opening {
        kind,
        bounds,
        kept,
        source,
    } = opening;
    close(kind, cx);
    let options = options(bounds, kind.is_task(), parent, cx);
    let over = parent.window_handle();
    let mut watching = Vec::new();
    let opened = cx.open_window(options, |window, cx| {
        #[cfg(target_os = "macos")]
        crate::native::window::float_on_its_space(window);
        cx.new(|cx| {
            if !kept {
                let popup = window.window_handle();
                let left = move |_: &mut V, _: &mut Window, cx: &mut Context<V>| {
                    settle(kind, popup, over, cx);
                };
                watching.push(cx.observe_window_activation(parent, left));
                watching.push(cx.observe_window_activation(window, left));
                // Moved or resized, the window under it would leave it hanging. GPUI says a
                // window's bounds changed as it's activated or left too, as when this one takes
                // the keyboard: that's not a move.
                let opened_over = parent.bounds();
                watching.push(cx.observe_window_bounds(parent, move |_, parent, cx| {
                    if parent.bounds() != opened_over {
                        close_if_up(kind, popup, cx);
                    }
                }));
            }
            build(window, cx)
        })
    });
    let handle = opened.ok()?;
    refresh_edge(handle.into(), cx);
    let up = Up {
        window: handle.into(),
        over,
        source,
        _watching: watching,
        settling: None,
    };
    cx.default_global::<Popups>().up.insert(kind, up);
    Some(handle)
}

/// The window of the floating window of `kind` that's up, if one is.
pub fn window(kind: Kind, cx: &App) -> Option<AnyWindowHandle> {
    Some(cx.try_global::<Popups>()?.up.get(&kind)?.window)
}

/// The view the floating window of `kind` that's up holds, if one is and it's a `V`.
pub fn root<V: Render>(kind: Kind, cx: &App) -> Option<&V> {
    window(kind, cx)?.downcast::<V>()?.read(cx).ok()
}

/// What the floating window of `kind` that's up was opened from, if one is and it says.
pub fn opened_from(kind: Kind, cx: &App) -> Option<ElementId> {
    let up = cx.try_global::<Popups>()?.up.get(&kind)?;
    Some(up.source.as_ref()?.id.clone())
}

/// Closes the floating window of `kind` that's up, if one is. Its window goes once nothing is in
/// it: what asks may be in that very window, as when it hears it's no longer the one in front,
/// and a window can't be reached from inside.
pub fn close(kind: Kind, cx: &mut App) {
    let Some(up) = cx.default_global::<Popups>().up.remove(&kind) else {
        return;
    };
    cx.defer(move |cx| {
        (up.window)
            .update(cx, |_, window, _| window.remove_window())
            .ok();
    });
}

/// Closes `popup` if it's still the floating window of `kind` that's up.
fn close_if_up(kind: Kind, popup: AnyWindowHandle, cx: &mut App) {
    if window(kind, cx) == Some(popup) {
        close(kind, cx);
    }
}

/// Closes `window`, a floating window, from inside it, forgetting it if it's the one of its kind
/// that's up: another may have opened since, and that one stays known.
pub fn close_from_inside(window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    cx.default_global::<Popups>()
        .up
        .retain(|_, up| up.window != handle);
    window.remove_window();
}

/// Hears every press in the window it's drawn in for [`pressed`], on the way down and whatever is
/// over the spot (a text field, a part that keeps the pointer to itself): drawn last in the
/// window's root, taking no room.
pub fn listener() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, (), window, _| {
            window.on_mouse_event(|event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    pressed(window.window_handle(), event.position, cx);
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// A press at `position` in the window `over`: every floating window over it goes, but one whose
/// trigger was pressed, which the trigger toggles.
pub fn pressed(over: AnyWindowHandle, position: Point<Pixels>, cx: &mut App) {
    let on_trigger =
        |up: &Up| (up.source.as_ref()).is_some_and(|source| source.drawn.get().contains(&position));
    close_over(over, |_, up| !on_trigger(up), cx);
}

/// The window `over` shows `what` now: if that's changed since it last said, the floating windows
/// over it go, but a dialog.
pub fn showing(over: AnyWindowHandle, what: &impl Hash, cx: &mut App) {
    let mut hasher = DefaultHasher::new();
    what.hash(&mut hasher);
    let now = hasher.finish();
    let before = cx.default_global::<Popups>().showing.insert(over, now);
    if before.is_some_and(|before| before != now) {
        close_over(over, |kind, _| !kind.is_task(), cx);
    }
}

/// Closes the floating windows over `over` that `closes` says go.
fn close_over(over: AnyWindowHandle, closes: impl Fn(Kind, &Up) -> bool, cx: &mut App) {
    let Some(popups) = cx.try_global::<Popups>() else {
        return;
    };
    let going: Vec<Kind> = (popups.up.iter())
        .filter(|(kind, up)| up.over == over && closes(**kind, up))
        .map(|(kind, _)| *kind)
        .collect();
    for kind in going {
        close(kind, cx);
    }
}

/// Closes `popup`, of `kind`, if, once activation has settled, neither it nor the window `over`
/// it is the one in front.
fn settle(kind: Kind, popup: AnyWindowHandle, over: AnyWindowHandle, cx: &mut App) {
    let look = cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(SETTLE))
            .await;
        cx.update(|cx| {
            // `App::active_window` knows main windows only, and a floating one is a panel.
            let in_front = |window: AnyWindowHandle, cx: &mut App| {
                (window.update(cx, |_, window, _| window.is_window_active())).unwrap_or(false)
            };
            if !in_front(popup, cx) && !in_front(over, cx) {
                close_if_up(kind, popup, cx);
            }
        });
    });
    let up = cx.default_global::<Popups>().up.get_mut(&kind);
    if let Some(up) = up.filter(|up| up.window == popup) {
        up.settling = Some(look);
    }
}

/// The corner a floating window opened from, which it keeps as it fits what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corner {
    pub top: bool,
    pub left: bool,
}

/// Fits the floating `window` to `extent`, keeping `corner` where it is.
pub fn fit(window: &mut Window, extent: Size<Pixels>, corner: Corner, cx: &mut App) {
    let now = window.bounds().size;
    let close = |a: Pixels, b: Pixels| (a - b).abs() < gpui_kit::px(0.5);
    if close(now.width, extent.width) && close(now.height, extent.height) {
        return;
    }
    #[cfg(target_os = "macos")]
    if crate::native::window::fit(window, extent, corner, cx) {
        return;
    }
    let _ = (corner, cx);
    window.resize(extent);
}

/// Makes the floating `window` `height` high, its top staying where it is, as a list under a
/// field grows and shrinks.
pub fn fit_height(height: f32, window: &mut Window, cx: &mut App) {
    let extent = gpui_kit::size(window.bounds().size.width, gpui_kit::px(height));
    let corner = Corner {
        top: true,
        left: true,
    };
    fit(window, extent, corner, cx);
}

/// Has a window just opened drawn with its edge as it looks once drawn, not as its first frame
/// left it (`native::window::refresh_edge`); off macOS there's nothing to do.
pub fn refresh_edge(window: AnyWindowHandle, cx: &mut App) {
    #[cfg(target_os = "macos")]
    crate::native::window::refresh_edge(window, cx);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, cx);
}

#[cfg(test)]
mod tests;

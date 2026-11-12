//! The app's floating windows: menus and their panels (`menu.rs`), dialogs (`modal.rs`), the
//! theme picker (`picker.rs`) and the sidebar's hover cards (`arc/card.rs`). Each is a glass
//! window over the window it belongs to, opened here, so they all float the same way:
//!
//! - It stays on the Space it opened on, and goes unseen while Koyo isn't the app in front. GPUI
//!   makes a pop-up panel join every Space, so one left open went with the person to another
//!   desktop and stayed over whatever they did there.
//! - Unless it's kept (a dialog the person leaves Koyo for and comes back to), it closes once
//!   neither it nor the window it belongs to is the one in front: the person went to another
//!   window, another app or another Space.
//! - Its edge is worked out again once its first frames are in (`refresh_edge`).

use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Pixels, Render, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
};

/// How long after a window's activation changes the leaving rule looks, in ms: the window gaining
/// it hears so only after the one losing it, as when a click on a menu's row moves it from the
/// window behind to the menu.
const SETTLE: u64 = 80;

/// What closes a floating window of a kind: that kind's own `close`.
pub type Close = fn(&mut App);

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

/// Opens the floating window `build` makes over `parent`, at `bounds` on its screen, taking the
/// keyboard if it should `focus`. `leaving` closes it once the person has left it; with none, it's
/// kept, only unseen while they're away from Koyo.
pub fn open<V: Render>(
    bounds: Bounds<Pixels>,
    focus: bool,
    leaving: Option<Close>,
    parent: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> Option<WindowHandle<V>> {
    let options = options(bounds, focus, parent, cx);
    let behind = parent.window_handle();
    let opened = cx.open_window(options, |window, cx| {
        #[cfg(target_os = "macos")]
        crate::native::window::float_on_its_space(window);
        cx.new(|cx| {
            if let Some(close) = leaving {
                let popup = window.window_handle();
                // Held by the floating window's view, so they go with it.
                let watch = move |_: &mut V, _: &mut Window, cx: &mut Context<V>| {
                    close_once_left(behind, popup, close, cx);
                };
                cx.observe_window_activation(parent, watch).detach();
                cx.observe_window_activation(window, watch).detach();
            }
            build(window, cx)
        })
    });
    let handle = opened.ok()?;
    refresh_edge(handle.into(), cx);
    Some(handle)
}

/// Closes `popup` with `close` if, once activation has settled, neither it nor the window
/// `behind` it is the one in front. One already gone, or replaced by another of its kind, is past
/// closing.
fn close_once_left(behind: AnyWindowHandle, popup: AnyWindowHandle, close: Close, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(SETTLE))
            .await;
        cx.update(|cx| {
            // `App::active_window` knows main windows only, and a floating one is a panel.
            let in_front = |window: AnyWindowHandle, cx: &mut App| {
                window.update(cx, |_, window, _| window.is_window_active())
            };
            let Ok(popup_in_front) = in_front(popup, cx) else {
                return;
            };
            if !popup_in_front && !in_front(behind, cx).unwrap_or(false) {
                close(cx);
            }
        });
    })
    .detach();
}

/// Has a window just opened drawn with its edge as it looks once drawn, not as its first frame
/// left it (`native::window::refresh_edge`); off macOS there's nothing to do.
pub fn refresh_edge(window: AnyWindowHandle, cx: &mut App) {
    #[cfg(target_os = "macos")]
    crate::native::window::refresh_edge(window, cx);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, cx);
}

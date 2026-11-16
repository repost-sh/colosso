//! Where named parts of Koyo's windows were last drawn, in a debug build, for the native pages'
//! harness to find them (`browser/native/probe.rs`, `bounds`): the chat's composer, a page's
//! address, a page, the tab switcher's panel. A release build notes none.

use gpui_kit::{Bounds, IntoElement, Pixels, Styled as _, Window, canvas};

#[cfg(debug_assertions)]
thread_local! {
    /// Each name's window, by GPUI's id of it, and its bounds there.
    static MARKS: std::cell::RefCell<std::collections::HashMap<String, (u64, Bounds<Pixels>)>> =
        std::cell::RefCell::default();
}

/// Notes that what's called `name` was drawn over `bounds` in `window`.
pub fn note(name: impl Into<String>, bounds: Bounds<Pixels>, window: &Window) {
    #[cfg(debug_assertions)]
    MARKS.with(|marks| {
        let id = window.window_handle().window_id().as_u64();
        marks.borrow_mut().insert(name.into(), (id, bounds));
    });
    #[cfg(not(debug_assertions))]
    let _ = (name.into(), bounds, window);
}

/// An element over all of its parent, which is laid out `relative`, noting where it's drawn as
/// `name`; none in a release build.
pub fn mark(name: &'static str) -> Option<impl IntoElement> {
    cfg!(debug_assertions).then(|| {
        canvas(move |bounds, window, _| note(name, bounds, window), |_, (), _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
    })
}

/// Where what's called `name` was last drawn: its window, by GPUI's id, and its bounds there.
#[cfg(all(target_os = "macos", debug_assertions))]
pub fn find(name: &str) -> Option<(u64, Bounds<Pixels>)> {
    MARKS.with(|marks| marks.borrow().get(name).copied())
}

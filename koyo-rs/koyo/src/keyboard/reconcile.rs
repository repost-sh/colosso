//! The one place that moves `AppKit`'s first responder: to what the window's holder means,
//! whenever the holder changes, the window becomes key, a page's views move, a key finds it
//! elsewhere, or something else moved it. `AppKit` is behind [`AppKit`], so tests give a fake.

use super::state::{Holder, PaneId, Surface, WindowKey};

/// A window's first responder, as the keyboard tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Responder {
    /// GPUI's own view.
    Gpui,
    /// One of the views of the page in pane `PaneId`: its keys' view, or another in it.
    Page(PaneId),
    /// The window itself: no view has the keyboard.
    Window,
    /// Something else: a native control, a view of Chromium's no page is known by.
    Other,
}

/// What the keyboard needs of `AppKit`, by window.
pub trait AppKit {
    /// `window`'s first responder now.
    fn first_responder(&self, window: WindowKey) -> Responder;
    /// Makes `to` the first responder of `window`; whether it is now. A page's is its keys' view,
    /// which only a window its views are in can have.
    fn make_first_responder(&self, window: WindowKey, to: Responder) -> bool;
    /// The window the page in `pane` has its views in, if any.
    fn page_window(&self, pane: PaneId) -> Option<WindowKey>;
    /// Whether `window`'s first responder is in the middle of composing text: a dead key or an
    /// input method's marked text, which a key belongs to.
    fn has_marked_text(&self, window: WindowKey) -> bool;
    /// Makes `window` the key window.
    fn make_key(&self, window: WindowKey);
    /// Whether `window` is the key window.
    fn is_key(&self, window: WindowKey) -> bool;
}

/// What the person's `surface` means in `window`: GPUI's view, or the page's keys' view while
/// its views are in that window. A page out of sight has none, and GPUI's view takes its place.
pub fn home(surface: Surface, window: WindowKey, appkit: &dyn AppKit) -> Responder {
    match surface {
        Surface::Page(pane) if appkit.page_window(pane) == Some(window) => Responder::Page(pane),
        Surface::Page(_) | Surface::Gpui(_) => Responder::Gpui,
    }
}

/// The first responder `holder` wants in `window`: none while a menu has the keys or they're
/// away, which leave it as it is.
pub fn wanted(holder: Holder, window: WindowKey, appkit: &dyn AppKit) -> Option<Responder> {
    match holder {
        Holder::Gpui(part) => Some(home(Surface::Gpui(part), window, appkit)),
        Holder::Page(pane) => Some(home(Surface::Page(pane), window, appkit)),
        Holder::Menu | Holder::Away => None,
    }
}

/// How `window`'s first responder moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    pub before: Responder,
    pub after: Responder,
}

/// Makes `to` the first responder of `window` unless it is; how it moved, if it was asked to.
pub fn make(to: Responder, window: WindowKey, appkit: &dyn AppKit) -> Option<Moved> {
    let before = appkit.first_responder(window);
    if before == to {
        return None;
    }
    appkit.make_first_responder(window, to);
    let after = appkit.first_responder(window);
    Some(Moved { before, after })
}

/// Makes what `holder` wants `window`'s first responder, unless it is.
pub fn reconcile(holder: Holder, window: WindowKey, appkit: &dyn AppKit) -> Option<Moved> {
    make(wanted(holder, window, appkit)?, window, appkit)
}

/// A key is about to be delivered in `window`: the first responder is made what `holder`
/// wants first, so the key goes there. Not in the middle of composing text, which the key
/// belongs to, wherever it is.
pub fn at_key(holder: Holder, window: WindowKey, appkit: &dyn AppKit) -> Option<Moved> {
    let wanted = wanted(holder, window, appkit)?;
    if appkit.first_responder(window) == wanted || appkit.has_marked_text(window) {
        return None;
    }
    make(wanted, window, appkit)
}

/// The person pressed GPUI's own content in `window`: if GPUI's view should have the keyboard
/// and hasn't, it has it before GPUI hears the press, so a press on the composer always gives
/// it the keys. With a page holding, the press is GPUI's to judge: one that moves GPUI's focus
/// moves the holder, and a button leaves the keyboard in the page.
pub fn at_press(holder: Holder, window: WindowKey, appkit: &dyn AppKit) -> Option<Moved> {
    match holder {
        Holder::Gpui(_) => make(Responder::Gpui, window, appkit),
        Holder::Page(_) | Holder::Menu | Holder::Away => None,
    }
}

/// What a change of `window`'s first responder to `after`, which the keyboard didn't make, is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// What's wanted, or left to whatever has the keys.
    Wanted,
    /// A page that the person didn't put the keyboard in asked for it: it's taken back.
    Refused(PaneId),
    /// Something else moved it off what's wanted: it's put back.
    Astray,
}

pub fn heard(
    holder: Holder,
    surface: Surface,
    after: Responder,
    window: WindowKey,
    appkit: &dyn AppKit,
) -> Heard {
    match (wanted(holder, window, appkit), after) {
        (_, Responder::Page(pane)) if surface == Surface::Page(pane) => Heard::Wanted,
        (_, Responder::Page(pane)) => Heard::Refused(pane),
        (Some(wanted), after) if after != wanted => Heard::Astray,
        _ => Heard::Wanted,
    }
}

/// What `window`'s first responder is put back to after a change it shouldn't have had: what
/// `holder` wants, or, while a menu has the keys or they're away, what the person's
/// `surface` means there, for when they're back.
pub fn restore(
    holder: Holder,
    surface: Surface,
    window: WindowKey,
    appkit: &dyn AppKit,
) -> Option<Moved> {
    let to = wanted(holder, window, appkit).unwrap_or_else(|| home(surface, window, appkit));
    make(to, window, appkit)
}

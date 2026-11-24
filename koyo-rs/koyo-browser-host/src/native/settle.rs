//! Where a page's views go: in the pane's view while a pane draws it in a window seen, parked
//! in its holder while an agent keeps it and no pane draws it, else out of every window, which
//! is what hides it (`docs/native-pages.md` 5).

use koyo_browser_ipc::PaneId;
use objc2::rc::Retained;
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::{COMPOSITOR, Views, WEB_CONTENTS, find_view, holder, page, tell_keyboard};

/// Puts a holder of `size` where it is while no pane shows its page: all but a point of it
/// past the screen's bottom left corner, where it still counts as on screen.
pub(super) fn park(holder: &NSWindow, size: NSSize) {
    let parked = NSRect::new(NSPoint::new(1. - size.width, 1. - size.height), size);
    if holder.frame() != parked {
        holder.setFrame_display(parked, false);
    }
}

/// Where a page's views go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Spot {
    /// In the app's view of the pane that draws the page: it's on screen for the engine.
    Pane,
    /// Parked in its holder off the screen, no pane drawing it: on screen for the engine all
    /// the same.
    Holder,
    /// Out of every window: hidden.
    Nowhere,
}

/// Where the views of a page go that a pane draws or not (`drawn`), that an agent keeps on
/// screen for the engine or not (`kept`), and whose pane's window counts as seen or not
/// (`seen`). The page is on screen for the engine while a pane draws it in a window seen, or
/// while it's kept.
pub(super) fn spot(drawn: bool, kept: bool, seen: bool) -> Spot {
    match (drawn, kept || seen) {
        (true, true) => Spot::Pane,
        (false, true) if kept => Spot::Holder,
        _ => Spot::Nowhere,
    }
}

/// Puts `pane`'s views where they belong now. The page is on screen for the engine while a
/// pane draws it in a window seen, or while an agent keeps it ([`sight`]): in the app's view
/// if a pane draws it, else parked in the holder off the screen. Else out of every window,
/// which is what hides it: Chromium is told not to judge by what covers a window (`app.rs`),
/// so a parked page is awake whatever is in front. The holder stays shown either way
/// (`docs/native-pages.md` 5); the windows Chromium ties to it go where the views are
/// ([`holder::rehome`]). Where a pane draws the page is the host's to know as the pane does
/// ([`place`], [`release`]): a page a pane stops drawing that no agent keeps leaves every
/// window at once, and is hidden once, not parked and woken first.
pub(super) fn settle(pane: PaneId) {
    let settled = page(pane, |entry| {
        let spot = spot(entry.shown_in.is_some(), entry.kept, entry.seen);
        let on_screen = spot != Spot::Nowhere;
        let changed = std::mem::replace(&mut entry.on_screen, on_screen) != on_screen;
        let made = (entry.holder.clone()).map(|(_, holder)| (holder, entry.views.clone(), entry.shown_in.clone(), entry.size));
        (changed, spot, made)
    });
    let Some((changed, spot, made)) = settled else {
        return;
    };
    if changed {
        crate::sight::shown(pane, spot != Spot::Nowhere);
    }
    let Some((holder, known, shown_in, size)) = made else {
        return;
    };
    let made = || {
        let content = holder.contentView()?;
        Some((find_view(&content, COMPOSITOR)?, find_view(&content, WEB_CONTENTS)?))
    };
    let Some((compositor, web)) = known.clone().or_else(made) else {
        return;
    };
    if known.is_none() {
        page(pane, |entry| entry.views = Some((compositor.clone(), web.clone())));
        // SAFETY: both are live views, on the main thread.
        unsafe {
            koyo_seam::pages::register(
                pane,
                Retained::as_ptr(&compositor).cast_mut().cast(),
                Retained::as_ptr(&web).cast_mut().cast(),
            );
        }
    }
    let into = |parent: &NSView| {
        let bounds = parent.bounds();
        for view in [&compositor, &web] {
            if !view.isDescendantOf(parent) {
                parent.addSubview(view);
                view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
            }
            if view.frame() != bounds {
                view.setFrame(bounds);
            }
        }
    };
    let to = match spot {
        Spot::Pane => shown_in.clone(),
        Spot::Holder => holder.contentView(),
        Spot::Nowhere => None,
    };
    let to_window = to.as_ref().and_then(|parent| parent.window());
    let leaving = web.window().is_some_and(|now| {
        (to_window.as_ref()).is_none_or(|to| !std::ptr::eq(&raw const *now, &raw const **to))
    });
    if leaving {
        tell_keyboard(pane, Views::Leaving);
    }
    let arriving = to_window.is_some();
    let from_window = web.window();
    if let Some(parent) = to {
        into(&parent);
    } else {
        compositor.removeFromSuperview();
        web.removeFromSuperview();
    }
    if shown_in.is_none() {
        park(&holder, size);
    }
    let apps = |window: &&NSWindow| !std::ptr::eq(std::ptr::from_ref(*window), &raw const *holder);
    holder::rehome(&holder, from_window.as_deref().filter(apps), to_window.as_deref().filter(apps));
    if arriving {
        tell_keyboard(pane, Views::Arrived);
    }
}

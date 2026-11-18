//! Whether a person can see what a window draws, the one answer everything that stops its work
//! out of sight goes by; and the elements that tell a thing they draw it there.

use std::cell::Cell;
use std::collections::HashSet;

use gpui_kit::{App, Context, EntityId, Subscription, WeakEntity, Window};

thread_local! {
    /// Whether a window nothing shows counts as seen all the same: the harness's (`tick`,
    /// `browser/native/probe.rs`), for an app driven on a desktop that isn't the one shown,
    /// where `AppKit` takes no window to be visible. Read in debug builds only, as the harness
    /// is there only, and on the main thread, where it and everything that asks run.
    static UNSEEN_COUNTS: Cell<bool> = const { Cell::new(false) };
}

/// Has a window nothing shows count as seen all the same, or no longer.
#[cfg(debug_assertions)]
pub(crate) fn count_unseen(on: bool) {
    UNSEEN_COUNTS.set(on);
}

/// Whether a window nothing shows counts as seen all the same ([`count_unseen`]).
pub(crate) fn unseen_counts() -> bool {
    cfg!(debug_assertions) && UNSEEN_COUNTS.get()
}

/// Whether a person can see what `window` draws: it's on screen, or counts as so.
pub(crate) fn window_seen(window: &Window) -> bool {
    window.is_visible() || unseen_counts()
}

/// The elements that draw a thing in a window seen, by their state's entity: it's in sight while
/// any does.
#[derive(Default)]
pub(crate) struct ShownBy(HashSet<EntityId>);

impl ShownBy {
    pub(crate) fn any(&self) -> bool {
        !self.0.is_empty()
    }

    /// Element `by` started or stopped drawing the thing in a window seen: whether it's in
    /// sight now, if that changed.
    pub(crate) fn set(&mut self, by: EntityId, shown: bool) -> Option<bool> {
        let was = self.any();
        if shown {
            self.0.insert(by);
        } else {
            self.0.remove(&by);
        }
        let now = self.any();
        (now != was).then_some(now)
    }
}

/// A thing elements draw, told as each starts or stops drawing it in a window seen.
pub(crate) trait Showable: 'static {
    fn set_shown_by(&mut self, by: EntityId, shown: bool);
}

/// What an element keeps in its state while it draws a [`Showable`]: the thing is shown by it
/// while its window is seen, and no longer once the element stops drawing it. A window nobody
/// sees draws nothing, and so lets go of nothing, so its visibility is followed too.
pub(crate) struct Shown {
    _seen: Subscription,
}

impl Shown {
    pub(crate) fn new<T: Showable>(
        thing: WeakEntity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let me = cx.entity_id();
        let tell = move |shown: bool, cx: &mut App| {
            thing.update(cx, |thing, _| thing.set_shown_by(me, shown)).ok();
        };
        tell(window_seen(window), cx);
        let seen = tell.clone();
        let _seen = cx.observe_window_visibility(window, move |_, _, window, cx| {
            seen(window_seen(window), cx);
        });
        cx.on_release(move |_: &mut Self, cx| tell(false, cx)).detach();
        Shown { _seen }
    }
}

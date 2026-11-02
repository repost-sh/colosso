//! What live sizing does to a pane's browser: overrides sent in the page's own session under
//! ids of the host's, the view resized, the app's own device metrics kept, and the timer
//! that ends a drag.

use std::time::Instant;

use cef::{ImplTask, Task, ThreadId, WrapTask, rc::Rc, wrap_task};
use colosso_browser_ipc::{PaneId, PixelSize, Viewport};

use super::{Do, Live, metrics};
use crate::host::inspect::{self, Unsent, Whose};
use crate::host::{Pane, Queued, enqueue, panes, with_pane};

/// The host's own commands in a page's session, by id, until they're answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Own {
    /// The override that keeps the layout while the view grows.
    Pin,
    /// Any other.
    Other,
}

/// Whether the page can be sized live: open, loaded (Chromium drops a session's commands
/// while its first page commits), and not `DevTools`.
fn can_live(p: &Pane) -> bool {
    p.browser.is_some() && p.loaded && p.inspector.is_none()
}

/// The app asks for `viewport`, `dragging` whether a drag of the pane's edge is under way:
/// whether live sizing took it. A drag starts it; the size that says the drag is over ends
/// it. When not taken, it's an ordinary resize, and a drag under way is cleared.
pub(crate) fn ask(pane: PaneId, viewport: Viewport, dragging: bool) -> bool {
    let now = Instant::now();
    let taken = with_pane(pane, |p| {
        if !can_live(p) {
            return Err(p.live.take().is_some());
        }
        match p.live.as_mut() {
            Some(live) if (live.scale() - viewport.scale).abs() > f32::EPSILON => {
                // Another display: the view takes the size as it is.
                p.live = None;
                Err(true)
            }
            Some(live) => Ok(live.ask(viewport, dragging, now)),
            // A size of its own (an agent's, a preset, the mode turned on): laid out as it is.
            None if !dragging => Err(false),
            None => {
                // The sizes come here from now on: one kept from before is never told.
                p.sizing.forget_next();
                let screen = crate::largest_screen();
                let (live, todo) = Live::start(p.viewport, viewport, screen, now);
                p.live = Some(live);
                Ok(todo)
            }
        }
    });
    match taken {
        Some(Ok(todo)) => {
            run(pane, &todo);
            settle_later(pane, super::SETTLE);
            true
        }
        Some(Err(cleared)) => {
            if cleared {
                run(pane, &[Do::Clear]);
            }
            false
        }
        None => false,
    }
}

/// Carries out `todo` on `pane`, now: from the UI thread outside CEF's callbacks.
pub(crate) fn run(pane: PaneId, todo: &[Do]) {
    for step in todo {
        match *step {
            Do::Override(size) => override_at(pane, size, Own::Other),
            Do::Pin(size) => override_at(pane, size, Own::Pin),
            Do::Resize(view) => panes::lay_out(pane, view),
            Do::Clear => own_metrics(pane),
        }
    }
    with_pane(pane, |p| {
        if p.live.as_ref().is_some_and(Live::done) {
            p.live = None;
        }
    });
}

/// Carries out `todo` once the callback under way has returned: CEF's observers and paint
/// callbacks mustn't be called back into.
fn run_later(pane: PaneId, todo: Vec<Do>) {
    if todo.is_empty() {
        return;
    }
    enqueue(Queued::Live { pane, todo });
}

/// The page painted at `size`, edge to edge, from within its paint.
pub(crate) fn painted(p: &mut Pane, pane: PaneId, size: PixelSize) {
    if let Some(live) = p.live.as_mut() {
        run_later(pane, live.painted(size, Instant::now()));
    }
}

/// The part of a frame of `size` the page fills.
pub(crate) fn filled(p: &Pane, size: PixelSize) -> PixelSize {
    match &p.live {
        Some(live) => {
            let laid = live.laid().pixels();
            PixelSize {
                width: size.width.min(laid.width),
                height: size.height.min(laid.height),
            }
        }
        None => size,
    }
}

/// An answer in the page's session to `id`: whether it was one of the host's own.
pub(crate) fn answered(pane: PaneId, id: i64, failed: bool) -> bool {
    let Some(Some(own)) = with_pane(pane, |p| p.own.remove(&id)) else {
        return false;
    };
    if own == Own::Pin {
        let todo = with_pane(pane, |p| match p.live.as_mut() {
            // A view grown without the layout pinned would show the page at the view's size.
            Some(_) if failed => {
                p.live = None;
                vec![Do::Clear]
            }
            Some(live) => live.pinned(),
            None => Vec::new(),
        });
        run_later(pane, todo.unwrap_or_default());
    }
    true
}

/// The app's own device metrics command for the page, as it goes to the page: kept, so the
/// override carries it and it comes back once a drag is over; while one is under way, laid
/// out at the drag's size.
pub(crate) fn app_command(
    pane: PaneId,
    method: &str,
    params: serde_json::Value,
) -> (String, serde_json::Value) {
    let set = method == "Emulation.setDeviceMetricsOverride";
    if !set && method != "Emulation.clearDeviceMetricsOverride" {
        return (method.to_owned(), params);
    }
    with_pane(pane, |p| {
        p.app_metrics = set.then(|| params.clone());
        let laid = p.live.as_ref().map(Live::laid);
        match laid {
            Some(size) => (
                "Emulation.setDeviceMetricsOverride".to_owned(),
                metrics(p.app_metrics.as_ref(), size),
            ),
            None => (method.to_owned(), params.clone()),
        }
    })
    .unwrap_or((method.to_owned(), params))
}

fn override_at(pane: PaneId, size: Viewport, own: Own) {
    let params = with_pane(pane, |p| metrics(p.app_metrics.as_ref(), size));
    if let Some(params) = params {
        send(pane, "Emulation.setDeviceMetricsOverride", params, own);
    }
}

/// The page's own metrics again: the app's, else none.
fn own_metrics(pane: PaneId) {
    match with_pane(pane, |p| p.app_metrics.clone()).flatten() {
        Some(params) => send(
            pane,
            "Emulation.setDeviceMetricsOverride",
            params,
            Own::Other,
        ),
        None => send(
            pane,
            "Emulation.clearDeviceMetricsOverride",
            serde_json::json!({}),
            Own::Other,
        ),
    }
}

fn send(pane: PaneId, method: &str, params: serde_json::Value, own: Own) {
    match inspect::send_in_session(pane, method, params, Whose::Host(own)) {
        Ok(()) | Err(Unsent::Closed) => {}
        Err(Unsent::NoSession | Unsent::Refused) => give_up(pane),
    }
}

/// The page's session doesn't take commands: the view is sized as it is asked, from now on.
fn give_up(pane: PaneId) {
    let shown = with_pane(pane, |p| p.live.take().map(|live| live.shown_viewport())).flatten();
    if let Some(shown) = shown {
        panes::lay_out(pane, shown);
    }
}

wrap_task! {
    struct Settle {
        pane: PaneId,
    }

    impl Task {
        fn execute(&self) {
            let pane = self.pane;
            let step = with_pane(pane, |p| {
                let live = p.live.as_mut()?;
                live.settling = false;
                Some(live.settle(Instant::now()))
            });
            match step.flatten() {
                Some(Ok(todo)) => {
                    run(pane, &todo);
                    settle_later(pane, super::SETTLE);
                }
                Some(Err(wait)) => settle_later(pane, wait),
                None => {}
            }
        }
    }
}

/// Asks the drag in `pane` whether it's over in `wait`, unless that's asked already.
fn settle_later(pane: PaneId, wait: std::time::Duration) {
    let posted = with_pane(pane, |p| {
        p.live
            .as_mut()
            .is_some_and(|live| !std::mem::replace(&mut live.settling, true))
    });
    if posted == Some(true) {
        let mut task = Settle::new(pane);
        cef::post_delayed_task(ThreadId::UI, Some(&mut task), panes::delay_ms(wait));
    }
}

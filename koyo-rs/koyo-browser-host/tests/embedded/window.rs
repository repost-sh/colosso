//! The app's window of a test's, off every screen so nothing shows on the person's, and the
//! pages shown in it as Koyo shows a pane's page: each in a view of its own, over what was there
//! before, and let go of as its tab is left.

use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Duration;

use koyo_browser_host::{embedded, native};
use koyo_browser_ipc::PaneId;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::bubbles::made;

/// How large the window is, and each page shown in it.
pub const SIZE: NSSize = NSSize::new(400., 300.);

/// The app's window and the views its pages are shown in, by pane.
type Shown = (Retained<NSWindow>, Vec<(PaneId, Retained<NSView>)>);

thread_local! {
    /// The app's window while a test has it, on the main thread.
    static WINDOW: RefCell<Option<Shown>> = const { RefCell::new(None) };
}

/// Runs `test` with the app's window there, and takes the window down after it.
pub fn within(test: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    on_main(|main| {
        let window = made(main, SIZE);
        window.orderBack(None);
        WINDOW.with(|kept| kept.replace(Some((window, Vec::new()))));
        Ok(())
    })?;
    let outcome = test();
    on_main(|_| {
        let (window, places) = WINDOW.with(RefCell::take).ok_or("no app's window")?;
        for (pane, place) in &places {
            native::release(*pane, place);
        }
        window.orderOut(None);
        Ok(())
    })?;
    outcome
}

/// Shows `pane`'s page in the window, over what it shows already, as a tab come to is drawn
/// where the one left was.
pub fn show(pane: PaneId) -> Result<(), String> {
    on_main(move |main| {
        WINDOW.with(|kept| {
            let mut kept = kept.borrow_mut();
            let (window, places) = kept.as_mut().ok_or("no app's window")?;
            let content = window.contentView().ok_or("the app's window has no view")?;
            let place = NSView::initWithFrame(main.alloc(), NSRect::new(NSPoint::ZERO, SIZE));
            content.addSubview(&place);
            native::place(pane, &place, &content, NSRect::new(window.frame().origin, SIZE));
            places.push((pane, place));
            Ok(())
        })
    })
}

/// Lets go of `pane`'s page, as the pane drawing it does once another tab is picked.
pub fn leave(pane: PaneId) -> Result<(), String> {
    on_main(move |_| {
        WINDOW.with(|kept| {
            let mut kept = kept.borrow_mut();
            let (_, places) = kept.as_mut().ok_or("no app's window")?;
            for (_, place) in places.extract_if(.., |(shown, _)| *shown == pane) {
                native::release(pane, &place);
                place.removeFromSuperview();
            }
            Ok(())
        })
    })
}

/// Runs `work` on the main thread, and waits for what it comes to.
pub fn on_main<T: Send + 'static>(
    work: impl FnOnce(MainThreadMarker) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (done, said) = mpsc::channel();
    embedded::on_main(move || {
        let outcome = MainThreadMarker::new().ok_or_else(|| "not on the main thread".to_owned()).and_then(work);
        let _ = done.send(outcome);
    });
    said.recv_timeout(Duration::from_secs(10))
        .map_err(|error| format!("the main thread didn't answer: {error}"))?
}

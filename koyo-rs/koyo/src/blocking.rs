//! The right to block: a file read or written, a path asked of the file system, a process
//! started or waited on. The main thread draws the frames and runs the browser engine, so
//! these happen only where a [`Blocking`] is in scope, and one is lent only for one call of a
//! closure: where the work runs off the main thread ([`spawn`], [`thread`]), as the app quits
//! ([`on_quit`]), or before the first window ([`at_launch`], `main.rs`'s alone). A function
//! that blocks takes `may: Blocking<'_>`, so its callers need one too. koyo-hygiene holds the
//! shipped build to it; `koyo-rs/blocking-debt.txt` lists the files that don't keep to it yet.
//! What still holds the main thread, a library's I/O among it, the stall meter logs (`stall`).

use std::io;
use std::marker::PhantomData;
use std::time::Instant;

use gpui_kit::{App, BackgroundExecutor, Task};

/// Proof that the thread it's lent on may block, for one call of the closure it's lent to: it
/// holds nothing, can't be kept past the call, nor sent to another thread.
#[derive(Clone, Copy)]
pub struct Blocking<'a> {
    call: PhantomData<&'a ()>,
    this_thread: PhantomData<*const ()>,
}

impl Blocking<'_> {
    fn lent() -> Self {
        Self {
            call: PhantomData,
            this_thread: PhantomData,
        }
    }
}

/// Runs `work` on the background executor, where blocking is fine.
pub fn spawn<T: Send + 'static>(
    executor: &BackgroundExecutor,
    work: impl FnOnce(Blocking<'_>) -> T + Send + 'static,
) -> Task<T> {
    executor.spawn(async move { work(Blocking::lent()) })
}

/// Runs `work` on a thread of its own named `name`, for work that waits on the file system
/// for as long as the app runs.
pub fn thread(name: &str, work: impl FnOnce(Blocking<'_>) + Send + 'static) -> io::Result<()> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || work(Blocking::lent()))
        .map(drop)
}

/// Runs `work` as the app quits, on the main thread, where what's due must land before the
/// process ends.
pub fn on_quit(cx: &mut App, mut work: impl FnMut(Blocking<'_>, &mut App) + 'static) {
    cx.on_app_quit(move |cx| {
        work(Blocking::lent(), cx);
        async {}
    })
    .detach();
}

/// Runs `work`, the reads `what` needs before the first window, on the main thread, and logs
/// how long it held the thread. `main.rs`'s alone.
pub fn at_launch<T>(what: &str, work: impl FnOnce(Blocking<'_>) -> T) -> T {
    let started = Instant::now();
    let made = work(Blocking::lent());
    log::info!(
        "{what} held the main thread {} ms",
        started.elapsed().as_millis()
    );
    made
}

/// Lends `work` the right to block, in a test or a visual check, where nothing draws.
#[cfg(any(test, feature = "visual-check"))]
pub fn in_test<T>(work: impl FnOnce(Blocking<'_>) -> T) -> T {
    work(Blocking::lent())
}

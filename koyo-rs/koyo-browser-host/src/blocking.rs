//! The right to block: a file read or written, a path asked of the file system, a socket made
//! or waited on. In Koyo's process the host runs on the app's main thread, which draws the
//! app's frames (`embedded.rs`), so these happen only where a [`Blocking`] is in scope, and one
//! is lent only for one call of a closure: on a thread of the host's own ([`thread`]), or in a
//! host process of its own before CEF runs ([`at_launch`], `lib.rs`'s alone). A function that
//! blocks takes `may: Blocking<'_>`, so its callers need one too. koyo-hygiene holds the
//! shipped build to it, as it holds the app to `koyo/src/blocking.rs`, whose `Blocking` the
//! app can't lend a library it doesn't build; `koyo-rs/blocking-debt.txt` lists the files that
//! don't keep to it yet.

use std::io;
use std::marker::PhantomData;
use std::thread::JoinHandle;

/// Proof that the thread it's lent on may block, for one call of the closure it's lent to: it
/// holds nothing, can't be kept past the call, nor sent to another thread.
#[derive(Clone, Copy)]
pub(crate) struct Blocking<'a> {
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

/// Runs `work` on a thread of its own named `name`.
pub(crate) fn thread(
    name: &str,
    work: impl FnOnce(Blocking<'_>) + Send + 'static,
) -> io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || work(Blocking::lent()))
}

/// Runs `work` in a host process of its own, on its main thread before CEF runs there and
/// after it has stopped: nothing of Koyo's waits on that thread. `lib.rs`'s alone.
pub(crate) fn at_launch<T>(work: impl FnOnce(Blocking<'_>) -> T) -> T {
    work(Blocking::lent())
}

/// Lends `work` the right to block, in a test.
#[cfg(test)]
pub(crate) fn in_test<T>(work: impl FnOnce(Blocking<'_>) -> T) -> T {
    work(Blocking::lent())
}

//! Cross-platform async channel primitives for native and WASM targets, and
//! cancellation for work handed to a background thread.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(not(target_family = "wasm"))]
pub use smol::channel::{Receiver, Sender, unbounded};

#[cfg(target_family = "wasm")]
pub use async_channel::{Receiver, Sender, unbounded};

/// Raises a flag when dropped, for a background job to stop at. A task that
/// awaits the job holds it, so the job stops once the task is dropped, as
/// when a newer job supersedes it: dropping the job's own task stops it only
/// before it starts.
#[derive(Debug, Default)]
pub struct CancelOnDrop(Arc<AtomicBool>);

impl CancelOnDrop {
    pub fn new() -> Self {
        Self::default()
    }

    /// The flag for the job to check.
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Whether the [`CancelOnDrop`] that handed out `flag` was dropped.
pub fn cancelled(flag: &AtomicBool) -> bool {
    flag.load(Ordering::Relaxed)
}

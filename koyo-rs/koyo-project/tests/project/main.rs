//! The project against real folders on disk and the platform's real watcher. Each test makes
//! its checkout under the target folder and removes it when done.

// Marked as test code, which this whole crate is, so the lints' leave for tests (to unwrap,
// to panic) reaches the helpers too.
#[cfg(test)]
mod common;
#[cfg(test)]
mod listing;
#[cfg(test)]
mod watching;

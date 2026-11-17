//! The project against real folders on disk and the platform's real watcher. Each test makes
//! its checkout under the target folder and removes it when done.

#[cfg(test)]
mod common;
#[cfg(test)]
mod listing;
#[cfg(test)]
mod watching;

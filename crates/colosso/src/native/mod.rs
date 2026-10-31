//! `AppKit` controls hosted in GPUI's window, where macOS should draw a control itself (its
//! Liquid Glass, its accent, its motion): each in a native surface of the window's
//! composition, which stacks it over the page and under GPUI's overlays, placed where the page
//! lays it out and clipped as the page clips it. A control says only how it's made, shown and
//! heard ([`Control`]); [`host`] does the rest, and everything unsafe is here.
//!
//! Adapted from `Glass-HQ/gpui` (Apache-2.0), `crates/gpui_macos/src/native_controls/`: the
//! targets controls send their actions to, ported to objc2 and to gpui-fast's native surfaces.

mod glass;
mod host;
mod surface;
mod target;

pub use glass::init;
pub use host::{Control, host};
pub use surface::Surface;

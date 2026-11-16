//! A native menu on the person's screen only when the person asked for it (`docs/browser.md`
//! 14): the gate's rule is the native layer's (`koyo_seam::menus`), which judges by the window's
//! last input as the layer heard it. Here is the gate's place in `AppKit`: the system's own ways
//! of popping a menu up, wrapped once for the process ([`install`]), `NSMenu`'s pop-up calls and
//! `NSPopUpButtonCell`'s click, which Chromium runs a `<select>`'s popup through.

mod appkit;
pub use appkit::{install, may_show};
pub use koyo_seam::menus::Refusal;

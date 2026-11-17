//! The seam between GPUI's window and the native views in it, a browser page's above all: who
//! has the keyboard, whose a mouse press is, which menus and panels may show, where a native
//! view sits and when it's in a window (`docs/native-layer.md`). GPUI takes the window for its
//! own and so do the views, and every rule about what happens where they meet is here, asked
//! through hooks by the code that touches `AppKit`.
//!
//! So far: where a key goes ([`keys`]), whose an input is ([`input`]), and the inputs the
//! process hears, kept under their labels ([`heard`]), and which native menus may show
//! ([`menus`]).

#[cfg(target_os = "macos")]
pub mod appkit;
pub mod heard;
pub mod input;
pub mod keyboard;
pub mod keys;
pub mod menus;
#[cfg(target_os = "macos")]
pub mod pages;

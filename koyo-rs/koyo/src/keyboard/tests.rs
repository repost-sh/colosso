//! The keyboard in GPUI's windows: what's GPUI's of it, tested with the shell in a test window.
//! Its rules are the native layer's, tested there (`koyo-seam`, `keyboard`).

#[cfg(target_os = "macos")]
mod characters;
mod inactive;
#[cfg(target_os = "macos")]
mod marks;

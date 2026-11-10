//! The desktop's picture (`docs/agent-desktop.md` 6): each window's latest frame, a surface
//! the host shares, looked up from the mach port it sent and drawn by GPUI as it is.

use std::collections::HashMap;

use gpui_kit::Context;
use koyo_browser_ipc::mach;
use koyo_desk_wire::link::WindowId;
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;
use objc2_io_surface::IOSurfaceRef;

use crate::browser::frames::pixel_buffer;

/// The desktop's picture: each window's latest frame. Apart from the desk, so a frame, sixty
/// of which can come in a second, redraws the tab and nothing that only watches how the desk
/// stands.
#[derive(Default)]
pub struct Picture {
    frames: HashMap<WindowId, CFRetained<CVPixelBuffer>>,
}

impl Picture {
    /// The latest frame of `window`.
    pub fn frame_of(&self, window: WindowId) -> Option<&CFRetained<CVPixelBuffer>> {
        self.frames.get(&window)
    }

    /// Takes windows' new frames.
    pub fn take(
        &mut self,
        latest: HashMap<WindowId, CFRetained<CVPixelBuffer>>,
        cx: &mut Context<Self>,
    ) {
        self.frames.extend(latest);
        cx.notify();
    }

    /// Drops the frames of all but `shown`: those of windows that are gone.
    pub fn keep(&mut self, shown: &[WindowId]) {
        self.frames.retain(|id, _| shown.contains(id));
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.frames.clear();
        cx.notify();
    }
}

/// The latest frame of each window in `rings`, which hold them in the order they came. Only
/// that one's surface is looked up from its port; every port is dropped. A frame is kept
/// whether or not its window is listed yet: frames and the list come by different ways, and
/// a still window sends its frame once, so one dropped for coming first would leave the
/// window blank. The next list drops what no window claims ([`Picture::keep`]).
pub fn latest(rings: Vec<mach::Ring>) -> HashMap<WindowId, CFRetained<CVPixelBuffer>> {
    let mut latest = HashMap::new();
    // Newest first: a window's older frames, which pile up while the app is busy, are let
    // go unread.
    for ring in rings.into_iter().rev() {
        let window = WindowId::try_from(ring.pane).ok();
        for port in ring.ports.into_iter().rev() {
            if let Some(window) = window.filter(|window| !latest.contains_key(window))
                && let Some(pixels) =
                    IOSurfaceRef::lookup_from_mach_port(port).and_then(|s| pixel_buffer(&s))
            {
                latest.insert(window, pixels);
            }
            mach::release(port);
        }
    }
    latest
}

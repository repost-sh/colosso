//! A picture of each tab, for the switcher's cards (`docs/tab-switcher.md` 4): what the window
//! last drew of the tab, shrunk. The renderer takes it
//! (`gpui_apple::metal_renderer::set_surface_capture`, which says how): a buffer painted over
//! the tab shown draws nothing, and has the window's pixels under it shrunk into its surface,
//! at most four times a second while the window is drawn anew, and once more as the switcher
//! opens. A second buffer over the same surface is what a card paints. So it's of any tab
//! alike, a terminal, a chat, a file, a page or a split; nothing is copied to the CPU, and
//! macOS asks no permission. A tab has one from the first time it's shown until it's closed,
//! in memory only.
//!
//! What's on glass, a chat's prompt, is in it, on a pane that stands for the glass, which
//! macOS draws and no frame of the window's holds (`native/surface.rs`).

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Once;

use gpui_apple::metal_renderer::set_surface_capture;
use gpui_kit::base::ElementExt as _;
use gpui_kit::{
    App, Bounds, Div, Global, IntoElement, Pixels, Styled as _, Window, canvas, point, px, size,
};
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_video::{CVPixelBuffer, kCVPixelFormatType_32BGRA};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};

use crate::browser::frames::pixel_buffer;
use crate::model::Workspace;

/// A picture's size in pixels: a card's picture's in points, twice over, for a display of two
/// pixels a point.
pub(super) const WIDTH: u32 = 312;
pub(super) const HEIGHT: u32 = 196;

/// A tab's picture: a small surface the renderer draws the tab into, shrunk, and the two buffers
/// over it.
struct Picture {
    /// Painted over the tab in the window, it has the picture taken of what's drawn there.
    taker: CFRetained<CVPixelBuffer>,
    /// Painted in a card, it shows the picture.
    shown: CFRetained<CVPixelBuffer>,
    /// How many times it was asked for at once (`retake`).
    asked: u32,
}

impl Picture {
    /// A picture not taken yet; none if the system gives no surface for it.
    fn new() -> Option<Picture> {
        let number = |value: u32| CFNumber::new_i64(i64::from(value));
        let (width, height, element) = (number(WIDTH), number(HEIGHT), number(4));
        let row = number((WIDTH * 4).next_multiple_of(64));
        // Blue, green, red and alpha, a byte each, as a frame's pixels are.
        let format = number(kCVPixelFormatType_32BGRA);
        // SAFETY: the keys are IOSurface's own property names.
        let keys: [&CFString; 5] = unsafe {
            [
                kIOSurfaceWidth,
                kIOSurfaceHeight,
                kIOSurfaceBytesPerElement,
                kIOSurfaceBytesPerRow,
                kIOSurfacePixelFormat,
            ]
        };
        let values: [&CFType; 5] = [&width, &height, &element, &row, &format];
        let properties = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
        // SAFETY: a dictionary of the documented creation properties.
        let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }?;
        let taker = pixel_buffer(&surface)?;
        set_surface_capture(&taker, 0);
        Some(Picture {
            taker,
            shown: pixel_buffer(&surface)?,
            asked: 0,
        })
    }
}

/// The pictures of the tabs shown since the app started.
#[derive(Default)]
struct Pictures {
    /// Each by its tab's first pane.
    of: HashMap<u64, Picture>,
    /// The tab shown, by its first pane, and all it's drawn in, as this frame laid it out
    /// ([`watched`]), until its taker is painted over it ([`taker`]).
    shown: Option<(u64, Bounds<Pixels>)>,
}

impl Global for Pictures {}

/// `area`, all the tab `shown` is drawn in, noting where each frame lays it out, for the tab's
/// picture to be taken there.
pub(super) fn watched(area: Div, shown: Option<u64>) -> Div {
    let Some(key) = shown else {
        return area;
    };
    area.on_prepaint(move |bounds, _, cx| {
        cx.default_global::<Pictures>().shown = Some((key, bounds));
    })
}

/// What takes the picture of the tab shown, where [`watched`] noted it this frame. It draws
/// nothing: the renderer takes the picture of what the window shows under the part of the tab
/// a card's picture is the shape of ([`framed`]).
///
/// It's painted from the window's root, taking no room, not from among the tab's panes: those
/// are drawn into a layer of their own, where the corners of the card they're on clip them, and
/// the renderer takes pictures of what's painted in the frame itself.
pub(super) fn taker() -> impl IntoElement {
    let paint = |_: Bounds<Pixels>, (), window: &mut Window, cx: &mut App| {
        let pictures = cx.default_global::<Pictures>();
        let Some((key, bounds)) = pictures.shown.take() else {
            return;
        };
        let picture = match pictures.of.entry(key) {
            Entry::Occupied(picture) => picture.into_mut(),
            Entry::Vacant(none) => {
                let Some(picture) = Picture::new() else {
                    static SAID: Once = Once::new();
                    SAID.call_once(|| log::warn!("no surface for a tab's picture"));
                    return;
                };
                none.insert(picture)
            }
        };
        let taker = picture.taker.clone();
        window.paint_surface(framed(bounds), taker);
    };
    canvas(|_, _, _| (), paint).absolute().size_0()
}

/// The picture of the tab known by `key`, to paint in a card: none until the tab has been shown.
pub(super) fn of(key: u64, cx: &App) -> Option<CFRetained<CVPixelBuffer>> {
    let pictures = cx.try_global::<Pictures>()?;
    Some(pictures.of.get(&key)?.shown.clone())
}

/// Has the picture of the tab known by `key` taken anew with the window's next frame, without
/// waiting its turn: as the switcher opens, which shows it.
pub(super) fn retake(key: u64, cx: &mut App) {
    if let Some(picture) = cx.default_global::<Pictures>().of.get_mut(&key) {
        picture.asked = picture.asked.wrapping_add(1);
        set_surface_capture(&picture.taker, picture.asked);
    }
}

/// The picture of the tab known by `key` as a check looks at it; none if the tab has none, or
/// it can't be read.
#[cfg(feature = "visual-check")]
pub(super) fn image(key: u64, cx: &App) -> Option<image::RgbaImage> {
    let shown = of(key, cx)?;
    let read = |pixel: &dyn Fn(usize, usize) -> [u8; 4]| {
        image::RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
            let [blue, green, red, alpha] = pixel(x as usize, y as usize);
            image::Rgba([red, green, blue, alpha])
        })
    };
    crate::browser::frames::with_pixels(&shown, WIDTH as usize, HEIGHT as usize, read)
}

/// Empties the picture of the tab known by `key`, if it has one, for a check to tell it's
/// taken anew.
#[cfg(feature = "visual-check")]
pub(super) fn blank(key: u64, cx: &App) {
    use objc2_core_video::CVPixelBufferGetIOSurface;
    use objc2_io_surface::IOSurfaceLockOptions;

    let shown = of(key, cx);
    let Some(surface) = shown.and_then(|shown| CVPixelBufferGetIOSurface(Some(&shown))) else {
        return;
    };
    let length = surface.height() * surface.bytes_per_row();
    let to_write = IOSurfaceLockOptions(0);
    // SAFETY: the surface is locked to be written while its bytes are, all of them within its
    // rows.
    unsafe {
        if surface.lock(to_write, std::ptr::null_mut()) == 0 {
            let first = surface.base_address().as_ptr().cast::<u8>();
            first.write_bytes(0, length);
            surface.unlock(to_write, std::ptr::null_mut());
        }
    }
}

/// Lets go of the pictures of the tabs `workspace` no longer has open: those no tab is known
/// by, its first pane.
pub(super) fn retain(workspace: &Workspace, cx: &mut App) {
    let kept = cx.try_global::<Pictures>().map(|pictures| &pictures.of);
    let Some(kept) = kept.filter(|kept| !kept.is_empty()) else {
        return;
    };
    let open: HashSet<u64> = workspace.tab_keys().collect();
    if kept.keys().any(|key| !open.contains(key)) {
        (cx.global_mut::<Pictures>().of).retain(|key, _| open.contains(key));
    }
}

/// The part of `bounds`, all a tab is drawn in, that its picture is of: its whole width from
/// its top down, as high as a picture's shape makes that; or, where it isn't that high, its
/// whole height across its middle.
fn framed(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let shape = WIDTH as f32 / HEIGHT as f32;
    let Bounds { origin, size: all } = bounds;
    if all.width / shape <= all.height {
        Bounds::new(origin, size(all.width, all.width / shape))
    } else {
        let across = all.height * shape;
        let left = (all.width - across) / 2.;
        Bounds::new(origin + point(left, px(0.)), size(across, all.height))
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, point, px, size};

    use super::framed;

    #[test]
    fn a_picture_is_of_the_top_of_a_tall_tab_and_the_middle_of_a_wide_one() {
        let on = |x: f32, y: f32, width: f32, height: f32| {
            Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
        };
        // Taller than a picture's shape, 312 by 196: all its width, from its top.
        assert_eq!(framed(on(300., 40., 624., 900.)), on(300., 40., 624., 392.));
        // Wider: all its height, across its middle.
        assert_eq!(framed(on(0., 0., 1000., 196.)), on(344., 0., 312., 196.));
    }
}

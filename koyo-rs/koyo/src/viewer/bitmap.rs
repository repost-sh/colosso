//! Bitmaps the viewer draws into with Quartz and hands to GPUI: four bytes a pixel, blue first,
//! in sRGB, as GPUI's sprites take them. Quartz draws with its colours multiplied by their
//! alpha, which GPUI's sprites don't expect, so a bitmap that may be see-through has that
//! taken back out.

use std::ffi::c_void;
use std::sync::Arc;

use gpui_kit::{App, RenderImage};
use image::{Delay, Frame, RgbaImage};
use objc2_core_foundation::{CFRetained, CGFloat, CGRect};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo,
    CGInterpolationQuality, kCGColorSpaceSRGB,
};

/// What a picture or a tile is drawn into: `width` by `height` pixels, and the context Quartz
/// draws them with, which writes straight into `pixels`.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pixels: Vec<u8>,
    context: CFRetained<CGContext>,
}

impl Canvas {
    /// A clear bitmap of `width` by `height`, or `None` when Quartz won't make one (a size of
    /// nothing, or more memory than there is).
    pub fn new(width: u32, height: u32) -> Option<Canvas> {
        if width == 0 || height == 0 {
            return None;
        }
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        // SAFETY: a constant Quartz defines.
        let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
        let info = CGImageByteOrderInfo::Order32Little.0 | CGImageAlphaInfo::PremultipliedFirst.0;
        // SAFETY: `pixels` is `width * 4` bytes a row for `height` rows, and outlives the
        // context, which the canvas drops first (`into_frame` takes the context away before it
        // gives the pixels up).
        let context = unsafe {
            CGBitmapContextCreate(
                pixels.as_mut_ptr().cast::<c_void>(),
                width as usize,
                height as usize,
                8,
                width as usize * 4,
                Some(&srgb),
                info,
            )
        }?;
        CGContext::set_interpolation_quality(Some(&context), CGInterpolationQuality::High);
        Some(Canvas {
            width,
            height,
            pixels,
            context,
        })
    }

    pub fn context(&self) -> &CGContext {
        &self.context
    }

    /// The whole bitmap, in Quartz's points, which are its pixels here.
    pub fn bounds(&self) -> CGRect {
        CGRect::new(
            objc2_core_foundation::CGPoint::new(0., 0.),
            objc2_core_foundation::CGSize::new(
                CGFloat::from(self.width),
                CGFloat::from(self.height),
            ),
        )
    }

    /// Fills the bitmap with opaque white, as paper.
    pub fn paper(&self) {
        CGContext::set_rgb_fill_color(Some(&self.context), 1., 1., 1., 1.);
        CGContext::fill_rect(Some(&self.context), self.bounds());
    }

    /// The pixels as a frame GPUI can show, held on screen for `delay` when it's one of an
    /// animation's. Quartz's multiplied colours are taken back out when `see_through`.
    pub fn into_frame(
        self,
        see_through: bool,
        delay: Option<std::time::Duration>,
    ) -> Option<Frame> {
        let Canvas {
            width,
            height,
            mut pixels,
            context,
        } = self;
        drop(context);
        if see_through {
            unpremultiply(&mut pixels);
        }
        let buffer = RgbaImage::from_raw(width, height, pixels)?;
        Some(match delay {
            Some(delay) => Frame::from_parts(buffer, 0, 0, Delay::from_saturating_duration(delay)),
            None => Frame::new(buffer),
        })
    }
}

/// Takes each pixel's colour back out of its alpha, as GPUI blends sprites with the colour
/// unmultiplied. Opaque and clear pixels are as they are.
pub fn unpremultiply(pixels: &mut [u8]) {
    pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .for_each(unpremultiplied);
}

/// One pixel's colour taken back out of its alpha.
pub fn unpremultiplied(pixel: &mut [u8; 4]) {
    let alpha = u32::from(pixel[3]);
    if alpha == 0 || alpha == 255 {
        return;
    }
    for channel in &mut pixel[..3] {
        *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
    }
}

/// Takes `images` out of the GPU's atlas of every window, once whatever's being drawn now is
/// done: what's dropped while a window is drawn isn't in the app's list of windows until then.
pub fn release(images: Vec<Arc<RenderImage>>, cx: &mut App) {
    if images.is_empty() {
        return;
    }
    cx.defer(move |cx| {
        for image in images {
            cx.drop_image(image, None);
        }
    });
}

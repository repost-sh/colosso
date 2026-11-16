//! A screen's picture on the GPU: a texture of ASTC blocks the renderer holds, the only copy
//! of the picture there is (`gpui_apple::metal_renderer::register_texture`), and the pixel a
//! chat paints to draw it, which stands for it and says how it looks (`set_surface_backdrop`).

use std::ptr::NonNull;

use gpui_apple::metal_renderer::{
    Backdrop, TextureFormat, register_texture, release_texture, set_surface_backdrop,
};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};

use super::astc::{Astc, Blocks};
use crate::settings::ChatBackdrop;

/// How a chat's backdrop is drawn from its picture, as the settings say: the dither's cells,
/// how many are lit, how much of it shows, and its fade. The renderer works it out as it
/// samples the picture, so none of it makes the picture again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    cell: f32,
    density: f32,
    opacity: f32,
    fade: Option<[f32; 2]>,
}

impl Look {
    pub fn of(settings: ChatBackdrop) -> Look {
        Look {
            cell: settings.cell.max(1.),
            density: settings.density.max(0.),
            opacity: settings.opacity.clamp(0., 1.),
            fade: settings.fade(),
        }
    }
}

/// A texture the renderer holds, let go when this is dropped.
pub struct Texture {
    id: u64,
    /// How many bytes it takes on the GPU.
    pub bytes: usize,
}

impl Texture {
    /// `picture` put on the GPU, from any thread; none when the GPU can't hold it.
    pub fn new(picture: &Astc) -> Option<Texture> {
        let format = match picture.blocks {
            Blocks::Four => TextureFormat::Astc4x4,
            Blocks::Eight => TextureFormat::Astc8x8,
        };
        let id = register_texture(format, picture.width, picture.height, &picture.data)?;
        Some(Texture {
            id,
            bytes: picture.data.len(),
        })
    }

    /// A pixel to stand for the texture, once it's marked with how it looks (`mark`).
    pub fn placeholder() -> Option<CFRetained<CVPixelBuffer>> {
        let mut buffer = std::ptr::null_mut();
        // SAFETY: `buffer` is a valid out-pointer; the default allocator, no attributes.
        let status = unsafe {
            let made = NonNull::from(&mut buffer);
            CVPixelBufferCreate(None, 1, 1, kCVPixelFormatType_32BGRA, None, made)
        };
        if status != kCVReturnSuccess {
            return None;
        }
        // SAFETY: a buffer created for the caller, who owns it.
        Some(unsafe { CFRetained::from_raw(NonNull::new(buffer)?) })
    }

    /// Has `placeholder`, painted over a rectangle, draw the texture there as `look` says.
    pub fn mark(&self, placeholder: &CVPixelBuffer, look: Look) {
        let backdrop = Backdrop {
            texture: self.id,
            cell: look.cell,
            density: look.density,
            opacity: look.opacity,
            fade: look.fade,
        };
        set_surface_backdrop(placeholder, backdrop);
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        release_texture(self.id);
    }
}

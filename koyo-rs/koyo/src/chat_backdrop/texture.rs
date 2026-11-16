//! A screen's picture on the GPU: a texture of ASTC blocks the renderer holds, the only copy
//! of the picture there is (`gpui_apple::metal_renderer::register_texture`), and the pixel a
//! chat paints to draw it, which stands for it (`set_surface_texture`).

use std::ptr::NonNull;

use gpui_apple::metal_renderer::{
    TextureFormat, register_texture, release_texture, set_surface_texture,
};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};

use super::astc::{Astc, Blocks};

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

    /// A pixel standing for the texture: painted over a rectangle, it draws the texture there.
    pub fn placeholder(&self) -> Option<CFRetained<CVPixelBuffer>> {
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
        let buffer = unsafe { CFRetained::from_raw(NonNull::new(buffer)?) };
        set_surface_texture(&buffer, self.id);
        Some(buffer)
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        release_texture(self.id);
    }
}

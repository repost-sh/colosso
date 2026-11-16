//! A screen's picture on the GPU: a texture of ASTC blocks the renderer holds, the only copy
//! of the picture there is (`gpui_apple::metal_renderer::register_texture`), and the pixel a
//! chat paints to draw it, which stands for it and says how it looks (`set_surface_backdrop`).
//! How it looks, the dither, the opacity and the fade, is the renderer's to work out as it
//! samples the picture, so none of it makes the picture again.

use gpui_apple::metal_renderer::{
    Backdrop, TextureFormat, register_texture, release_texture, set_surface_backdrop,
};
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;

use super::astc::{Astc, Blocks};
use crate::native::surface::marker;
use crate::settings::ChatBackdrop;

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

    /// A pixel to stand for a texture, once it's marked with how it looks (`mark`). From the
    /// main thread, where it's painted.
    pub fn placeholder() -> Option<CFRetained<CVPixelBuffer>> {
        marker()
    }

    /// Has `placeholder`, painted over a rectangle, draw the texture there as `settings` say.
    pub fn mark(&self, placeholder: &CVPixelBuffer, settings: &ChatBackdrop) {
        let backdrop = Backdrop {
            texture: self.id,
            cell: settings.cell,
            density: settings.density,
            opacity: settings.opacity,
            fade: settings.fade(),
        };
        set_surface_backdrop(placeholder, backdrop);
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        release_texture(self.id);
    }
}

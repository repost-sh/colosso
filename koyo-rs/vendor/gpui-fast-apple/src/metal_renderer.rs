// Modified by Longbridge for gpui-fast.
use crate::metal_atlas::MetalAtlas;
use anyhow::{Context as _, Result};
use block2::RcBlock;
use gpui::{
    AtlasTextureId, Background, Bounds, ContentMask, Corners, DevicePixels, Edges, Hsla,
    PaintSurface, Path, Point, PrimitiveBatch, Quad, ScaledPixels, Scene, Size, point, size,
};
#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
use image::RgbaImage;
use objc2::{Message, rc::Retained, runtime::ProtocolObject};
use objc2_core_foundation::{CFData, CFRetained, CFString, CGSize};
use objc2_core_graphics::{CGColorSpace, kCGColorSpaceDisplayP3};
use objc2_core_video::{
    CVAttachmentMode, CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer,
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetWidth, CVPixelBufferGetWidthOfPlane, kCVPixelFormatType_32BGRA,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, kCVReturnSuccess,
};
use objc2_foundation::{NSRange, NSString};
#[cfg(target_os = "macos")]
use objc2_metal::MTLCopyAllDevices;
use objc2_metal::{
    MTLBlendFactor, MTLBlendOperation, MTLBlitCommandEncoder, MTLBuffer, MTLClearColor,
    MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice,
    MTLDrawable, MTLGPUFamily, MTLLibrary, MTLLoadAction, MTLOrigin, MTLPixelFormat,
    MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLResourceOptions, MTLSize,
    MTLStorageMode, MTLStoreAction, MTLTexture, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage, MTLViewport,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use parking_lot::Mutex;

use std::{
    cell::{Cell, RefCell},
    iter, mem,
    mem::MaybeUninit,
    ops::Range,
    ptr,
    rc::Rc,
    slice,
    sync::Arc,
    time::{Duration, Instant},
};

// Exported to metal
pub(crate) type PointF = gpui::Point<f32>;

#[cfg(not(feature = "runtime_shaders"))]
const SHADERS_METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shaders.metallib"));
#[cfg(feature = "runtime_shaders")]
const SHADERS_SOURCE_FILE: &str = include_str!(concat!(env!("OUT_DIR"), "/stitched_shaders.metal"));
// Use 4x MSAA, all devices support it.
// https://developer.apple.com/documentation/metal/mtldevice/1433355-supportstexturesamplecount
const PATH_SAMPLE_COUNT: u32 = 4;
/// Metal requires the offset a buffer is bound at to be 256-byte aligned.
const INSTANCE_BUFFER_ALIGNMENT: usize = 256;
const MAX_INSTANCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;

pub type Context = Arc<Mutex<InstanceBufferPool>>;
pub type Renderer = MetalRenderer;

pub fn new_renderer(
    context: self::Context,
    transparent: bool,
    use_display_p3: bool,
) -> Result<Renderer> {
    MetalRenderer::new(context, transparent, use_display_p3)
}

pub fn new_overlay_renderer(context: self::Context, base: &Renderer) -> Result<Renderer> {
    crate::fast::composition::new_overlay_renderer(context, base)
}

pub struct InstanceBufferPool {
    buffer_size: usize,
    buffers: Vec<InstanceBuffer>,
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self {
            buffer_size: 2 * 1024 * 1024,
            buffers: Vec::new(),
        }
    }
}

pub(crate) struct InstanceBuffer {
    metal_buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
    size: usize,
}

// SAFETY: Each buffer has one CPU writer and returns to the mutex-protected pool only after GPU completion.
unsafe impl Send for InstanceBuffer {}

impl InstanceBufferPool {
    pub(crate) fn reset(&mut self, buffer_size: usize) {
        self.buffer_size = buffer_size;
        self.buffers.clear();
    }

    pub(crate) fn acquire(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        unified_memory: bool,
    ) -> Result<InstanceBuffer> {
        if let Some(buffer) = self.buffers.pop() {
            return Ok(buffer);
        }
        let options = if unified_memory {
            MTLResourceOptions::StorageModeShared
                // Buffers are write only which can benefit from the combined cache
                // https://developer.apple.com/documentation/metal/mtlresourceoptions/cpucachemodewritecombined
                | MTLResourceOptions::CPUCacheModeWriteCombined
        } else {
            MTLResourceOptions::StorageModeManaged
        };
        let buffer = device
            .newBufferWithLength_options(self.buffer_size, options)
            .with_context(|| format!("allocating {}-byte instance buffer", self.buffer_size))?;
        Ok(InstanceBuffer {
            metal_buffer: buffer,
            size: self.buffer_size,
        })
    }

    pub(crate) fn release(&mut self, buffer: InstanceBuffer) {
        if buffer.size == self.buffer_size {
            self.buffers.push(buffer)
        }
    }
}

enum DisplayLinkMode {
    Detached,
    Attached {
        drawable: Option<Retained<ProtocolObject<dyn CAMetalDrawable>>>,
    },
}

pub struct MetalRenderer {
    pub(crate) device: Retained<ProtocolObject<dyn MTLDevice>>,
    pub(crate) layer: Option<Retained<CAMetalLayer>>,
    is_apple_gpu: bool,
    pub(crate) is_unified_memory: bool,
    presents_with_transaction: bool,
    display_link_mode: DisplayLinkMode,
    /// For headless rendering, tracks whether output should be opaque
    pub(crate) opaque: bool,
    pub(crate) command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pub(crate) paths_rasterization_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    path_sprites_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    shadows_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    quads_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    underlines_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    monochrome_sprites_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pub(crate) polychrome_sprites_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    surfaces_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Koyo: BGRA surfaces with a picture (`set_surface_picture`).
    page_surfaces_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Koyo: a chat's backdrop (`set_surface_backdrop`).
    backdrop_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Koyo: the BGRA surfaces the frame being encoded samples, and their textures, held until
    /// the GPU is done with the frame. A pixel buffer made over an `IOSurface` marks the surface
    /// in use while it lives, which every process sees: so the process that shares the surface
    /// (a browser page's host) doesn't paint into it while the GPU still reads it.
    page_surfaces_in_flight: Vec<(
        CFRetained<CVPixelBuffer>,
        Retained<ProtocolObject<dyn MTLTexture>>,
    )>,
    /// Koyo: the pictures taken of parts of a frame (`set_surface_capture`).
    pictures: Pictures,
    pub(crate) unit_vertices: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub(crate) instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    pub(crate) sprite_atlas: Arc<MetalAtlas>,
    core_video_texture_cache: CFRetained<CVMetalTextureCache>,
    pub(crate) path_intermediate_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    pub(crate) path_intermediate_msaa_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    path_sample_count: u32,
    /// Offscreen render target reused across `render_scene` calls when
    /// rendering headlessly without reading pixels back.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    headless_render_target: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    pub(crate) fast_layers: crate::fast::layers::TileCache,
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    /// Creates a new MetalRenderer with a CAMetalLayer for window-based rendering.
    pub fn new(
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
        transparent: bool,
        use_display_p3: bool,
    ) -> Result<Self> {
        let device = Self::create_device()?;
        let layer = CAMetalLayer::new();
        Self::configure_layer(&layer, &device, transparent, use_display_p3);
        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    /// Creates a renderer for a CAMetalLayer owned by a platform view, such as
    /// the backing layer UIKit creates for a view whose `layerClass` is
    /// `CAMetalLayer`. The renderer retains the layer.
    pub fn from_layer(
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
        layer: &CAMetalLayer,
        transparent: bool,
    ) -> Result<Self> {
        let device = Self::create_device()?;
        let layer = layer.retain();
        Self::configure_layer(&layer, &device, transparent, false);
        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    pub(crate) fn configure_layer(
        layer: &CAMetalLayer,
        device: &ProtocolObject<dyn MTLDevice>,
        transparent: bool,
        use_display_p3: bool,
    ) {
        layer.setDevice(Some(device));
        layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        if use_display_p3 {
            // SAFETY: The constant is extern, from Core Graphics
            let display_p3_name = unsafe { kCGColorSpaceDisplayP3 };
            if let Some(display_p3_color_space) = CGColorSpace::with_name(Some(display_p3_name)) {
                layer.setColorspace(Some(&display_p3_color_space));
            } else {
                log::error!("unable to create the Display P3 color space");
            }
        }
        // Support direct-to-display rendering if the window is not transparent
        // https://developer.apple.com/documentation/metal/managing-your-game-window-for-metal-in-macos
        layer.setOpaque(!transparent);
        layer.setMaximumDrawableCount(3);
        // Allow texture reading for visual tests and UI automation screenshots
        // (captures without ScreenCaptureKit). Only debug builds pay the
        // presentation cost, even when `test-support` is compiled in.
        #[cfg(all(feature = "test-support", debug_assertions))]
        layer.setFramebufferOnly(false);
        layer.setAllowsNextDrawableTimeout(false);
        layer.setNeedsDisplayOnBoundsChange(true);
        // UIKit sizes a view's backing layer itself; only AppKit-hosted
        // layers need to track their superlayer's bounds.
        #[cfg(target_os = "macos")]
        layer.setAutoresizingMask(
            objc2_quartz_core::CAAutoresizingMask::LayerWidthSizable
                | objc2_quartz_core::CAAutoresizingMask::LayerHeightSizable,
        );
    }

    /// Creates a new headless MetalRenderer for offscreen rendering without a window.
    ///
    /// This renderer can render scenes to images without requiring a CAMetalLayer,
    /// window, or AppKit. Use `render_scene_to_image()` to render scenes.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn new_headless(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Result<Self> {
        let device = Self::create_device()?;
        Self::new_internal(device, None, true, instance_buffer_pool)
    }

    #[cfg(target_os = "macos")]
    fn create_device() -> Result<Retained<ProtocolObject<dyn MTLDevice>>> {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        if let Some(device) = MTLCopyAllDevices()
            .iter()
            .min_by_key(|device| (device.isRemovable(), !device.isLowPower()))
        {
            Ok(device)
        } else {
            // For some reason `all()` can return an empty list, see https://github.com/zed-industries/zed/issues/37689
            // In that case, we fall back to the system default device.
            log::error!(
                "Unable to enumerate Metal devices; attempting to use system default device"
            );
            Self::system_default_device()
        }
    }

    #[cfg(target_os = "ios")]
    fn create_device() -> Result<Retained<ProtocolObject<dyn MTLDevice>>> {
        Self::system_default_device()
    }

    fn system_default_device() -> Result<Retained<ProtocolObject<dyn MTLDevice>>> {
        MTLCreateSystemDefaultDevice().context("accessing a compatible Metal device")
    }

    pub(crate) fn new_internal(
        device: Retained<ProtocolObject<dyn MTLDevice>>,
        layer: Option<Retained<CAMetalLayer>>,
        opaque: bool,
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    ) -> Result<Self> {
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .newLibraryWithSource_options_error(
                &NSString::from_str(SHADERS_SOURCE_FILE),
                Some(&objc2_metal::MTLCompileOptions::new()),
            )
            .context("building Metal library")?;
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .newLibraryWithData_error(&dispatch2::DispatchData::from_static_bytes(
                SHADERS_METALLIB,
            ))
            .context("loading Metal library")?;

        fn to_float2_bits(point: PointF) -> u64 {
            let mut output = point.y.to_bits() as u64;
            output <<= 32;
            output |= point.x.to_bits() as u64;
            output
        }

        // Shared memory can be used only if CPU and GPU share the same memory space.
        // https://developer.apple.com/documentation/metal/setting-resource-storage-modes
        // iOS does not support managed resources. Its simulator may report a
        // non-unified host GPU even though resources must still use shared storage.
        let is_unified_memory = cfg!(target_os = "ios") || device.hasUnifiedMemory();
        // Apple GPU families support memoryless textures, which can significantly reduce
        // memory usage by keeping render targets in on-chip tile memory instead of
        // allocating backing store in system memory.
        // https://developer.apple.com/documentation/metal/mtlgpufamily
        let is_apple_gpu = device.supportsFamily(MTLGPUFamily::Apple1);

        let unit_vertices = [
            to_float2_bits(point(0., 0.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(1., 1.)),
        ];
        // SAFETY: Metal copies the live array using its exact byte length.
        let unit_vertices = unsafe {
            device.newBufferWithBytes_length_options(
                ptr::NonNull::from(&unit_vertices).cast(),
                mem::size_of_val(&unit_vertices),
                if is_unified_memory {
                    MTLResourceOptions::StorageModeShared
                        | MTLResourceOptions::CPUCacheModeWriteCombined
                } else {
                    MTLResourceOptions::StorageModeManaged
                },
            )
        }
        .context("allocating unit vertex buffer")?;

        let paths_rasterization_pipeline_state = build_path_rasterization_pipeline_state(
            &device,
            &library,
            "paths_rasterization",
            "path_rasterization_vertex",
            "path_rasterization_fragment",
            MTLPixelFormat::BGRA8Unorm,
            PATH_SAMPLE_COUNT,
        )?;
        let path_sprites_pipeline_state = build_path_sprite_pipeline_state(
            &device,
            &library,
            "path_sprites",
            "path_sprite_vertex",
            "path_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;
        let shadows_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "shadows",
            "shadow_vertex",
            "shadow_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;
        let quads_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "quads",
            "quad_vertex",
            "quad_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;
        let underlines_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "underlines",
            "underline_vertex",
            "underline_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;
        let monochrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "monochrome_sprites",
            "monochrome_sprite_vertex",
            "monochrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;
        let polychrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "polychrome_sprites",
            "polychrome_sprite_vertex",
            "polychrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;
        let surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "surfaces",
            "surface_vertex",
            "surface_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;
        let page_surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "page_surfaces",
            "page_surface_vertex",
            "page_surface_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;
        let backdrop_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "backdrop",
            "backdrop_vertex",
            "backdrop_fragment",
            MTLPixelFormat::BGRA8Unorm,
        )?;

        let command_queue = crate::fast::dynamic_textures::command_queue(&device)?;
        let supports_shared_storage = cfg!(target_os = "ios") || is_apple_gpu;
        #[expect(
            clippy::arc_with_non_send_sync,
            reason = "PlatformWindow::sprite_atlas requires Arc, not Send or Sync"
        )]
        let sprite_atlas = Arc::new(MetalAtlas::new(device.clone(), supports_shared_storage));
        crate::fast::dynamic_textures::init(device.clone(), supports_shared_storage);
        let mut cache = ptr::null_mut();
        // SAFETY: The output pointer is writable and both optional attribute dictionaries are absent.
        let status = unsafe {
            CVMetalTextureCache::create(None, None, &device, None, ptr::NonNull::from(&mut cache))
        };
        anyhow::ensure!(
            status == kCVReturnSuccess,
            "creating CoreVideo Metal texture cache: status {status}"
        );
        let cache = ptr::NonNull::new(cache).context("retrieving CoreVideo Metal texture cache")?;
        // SAFETY: The successful create call transfers ownership of this non-null cache.
        let core_video_texture_cache = unsafe { CFRetained::from_raw(cache) };

        Ok(Self {
            device,
            layer,
            presents_with_transaction: false,
            display_link_mode: DisplayLinkMode::Detached,
            is_apple_gpu,
            is_unified_memory,
            opaque,
            command_queue,
            paths_rasterization_pipeline_state,
            path_sprites_pipeline_state,
            shadows_pipeline_state,
            quads_pipeline_state,
            underlines_pipeline_state,
            monochrome_sprites_pipeline_state,
            polychrome_sprites_pipeline_state,
            surfaces_pipeline_state,
            page_surfaces_pipeline_state,
            backdrop_pipeline_state,
            page_surfaces_in_flight: Vec::new(),
            pictures: Pictures::default(),
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
            core_video_texture_cache,
            path_intermediate_texture: None,
            path_intermediate_msaa_texture: None,
            path_sample_count: PATH_SAMPLE_COUNT,
            #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
            headless_render_target: None,
            fast_layers: crate::fast::layers::TileCache::default(),
        })
    }

    pub fn layer(&self) -> Option<&CAMetalLayer> {
        self.layer.as_deref()
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        self.layer
            .as_ref()
            .map(|layer| Retained::as_ptr(layer).cast_mut())
            .unwrap_or(ptr::null_mut())
    }

    pub fn sprite_atlas(&self) -> &Arc<MetalAtlas> {
        &self.sprite_atlas
    }

    /// Draw the next frame into `drawable` instead of asking the layer for one.
    ///
    /// A `CAMetalDisplayLink` hands out each frame's drawable early enough for
    /// the frame to reach the display on time; `nextDrawable` would instead
    /// wait for the compositor to release one. Pass `None` once the frame is
    /// done, so a drawable the frame did not use goes back to the layer.
    ///
    /// Returns the drawable this replaces: after a frame, `Some` means the
    /// frame neither drew nor presented.
    ///
    /// This is a setter rather than a guard scoped to one frame because the
    /// window releases its lock on the renderer while GPUI draws.
    pub fn set_frame_drawable(
        &mut self,
        drawable: Option<Retained<ProtocolObject<dyn CAMetalDrawable>>>,
    ) -> Option<Retained<ProtocolObject<dyn CAMetalDrawable>>> {
        match &mut self.display_link_mode {
            DisplayLinkMode::Attached { drawable: current } => mem::replace(current, drawable),
            DisplayLinkMode::Detached => {
                // Only a live link hands out drawables, and a link is detached
                // only after it has been invalidated.
                debug_assert!(
                    drawable.is_none(),
                    "received a display link drawable while detached"
                );
                None
            }
        }
    }

    /// Record whether a `CAMetalDisplayLink` drives the layer. While one
    /// does, frames are drawn only into the drawables it hands out.
    pub fn set_display_link_attached(&mut self, attached: bool) {
        self.display_link_mode = if attached {
            DisplayLinkMode::Attached { drawable: None }
        } else {
            DisplayLinkMode::Detached
        };
    }

    pub fn is_display_link_attached(&self) -> bool {
        matches!(self.display_link_mode, DisplayLinkMode::Attached { .. })
    }

    pub fn presents_with_transaction(&self) -> bool {
        self.presents_with_transaction
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        if let Some(layer) = &self.layer {
            layer.setPresentsWithTransaction(presents_with_transaction);
        }
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        if let Some(layer) = &self.layer {
            layer.setDrawableSize(CGSize::new(size.width.0 as f64, size.height.0 as f64));
        }
        if let Err(error) = self.update_path_intermediate_textures(size) {
            log::error!("failed to resize path textures: {error:#}");
        }
        crate::fast::layers::TileCache::clear(&mut self.fast_layers);
    }

    pub(crate) fn update_path_intermediate_textures(&mut self, size: Size<DevicePixels>) -> Result<()> {
        // We are uncertain when this happens, but sometimes size can be 0 here. Most likely before
        // the layout pass on window creation. Zero-sized texture creation causes SIGABRT.
        // https://github.com/zed-industries/zed/issues/36229
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            return Ok(());
        }

        if self
            .path_intermediate_texture
            .as_ref()
            .is_some_and(|texture| {
                texture.width() == size.width.0 as usize
                    && texture.height() == size.height.0 as usize
            })
        {
            return Ok(());
        }

        self.path_intermediate_texture = None;
        self.path_intermediate_msaa_texture = None;
        let texture_descriptor = MTLTextureDescriptor::new();
        // SAFETY: The dimensions are positive and this descriptor creates a 2D texture.
        unsafe {
            texture_descriptor.setWidth(size.width.0 as usize);
            texture_descriptor.setHeight(size.height.0 as usize);
        }
        texture_descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        texture_descriptor.setStorageMode(MTLStorageMode::Private);
        texture_descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        let intermediate_texture = self
            .device
            .newTextureWithDescriptor(&texture_descriptor)
            .context("allocating path intermediate texture")?;

        let msaa_texture = if self.path_sample_count > 1 {
            // https://developer.apple.com/documentation/metal/choosing-a-resource-storage-mode-for-apple-gpus
            // Rendering MSAA textures are done in a single pass, so we can use memory-less storage on Apple Silicon
            let storage_mode = if self.is_apple_gpu {
                MTLStorageMode::Memoryless
            } else {
                MTLStorageMode::Private
            };

            let msaa_descriptor = texture_descriptor;
            msaa_descriptor.setTextureType(MTLTextureType::Type2DMultisample);
            msaa_descriptor.setStorageMode(storage_mode);
            // SAFETY: The multisample texture uses the same sample count as its render pipeline.
            unsafe {
                msaa_descriptor.setSampleCount(self.path_sample_count as _);
            }
            Some(
                self.device
                    .newTextureWithDescriptor(&msaa_descriptor)
                    .context("allocating multisample path texture")?,
            )
        } else {
            None
        };
        self.path_intermediate_texture = Some(intermediate_texture);
        self.path_intermediate_msaa_texture = msaa_texture;
        Ok(())
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        self.opaque = !transparent;
        if let Some(layer) = &self.layer {
            layer.setOpaque(!transparent);
        }
    }

    pub fn destroy(&self) {
        // nothing to do
    }

    pub fn draw(&mut self, scene: &Scene) {
        let layer = match &self.layer {
            Some(l) => l.clone(),
            None => {
                log::error!(
                    "draw() called on headless renderer - use render_scene_to_image() instead"
                );
                return;
            }
        };
        let drawable = match &mut self.display_link_mode {
            DisplayLinkMode::Attached { drawable } => {
                let Some(drawable) = drawable.take() else {
                    log::error!("skipped a frame drawn outside its display link's update");
                    return;
                };
                drawable
            }
            DisplayLinkMode::Detached => {
                let Some(drawable) = layer.nextDrawable() else {
                    log::error!(
                        "failed to retrieve next drawable, drawable size: {:?}",
                        layer.drawableSize()
                    );
                    return;
                };
                drawable
            }
        };
        // A display link's drawable was sized when the link handed it out,
        // which can be before a resize the layer has since taken.
        let texture = drawable.texture();
        let viewport_size: Size<DevicePixels> = size(
            (texture.width() as i32).into(),
            (texture.height() as i32).into(),
        );

        let command_buffer = match self.render_frame(scene, &drawable.texture(), viewport_size) {
            Ok(command_buffer) => command_buffer,
            Err(error) => {
                log::error!("failed to render: {error:#}");
                return;
            }
        };

        if self.presents_with_transaction {
            command_buffer.commit();
            command_buffer.waitUntilScheduled();
            drawable.present();
        } else {
            command_buffer.presentDrawable(ProtocolObject::from_ref(&*drawable));
            command_buffer.commit();
        }
    }

    pub(crate) fn render_frame(
        &mut self,
        scene: &Scene,
        texture: &ProtocolObject<dyn MTLTexture>,
        viewport_size: Size<DevicePixels>,
    ) -> Result<Retained<ProtocolObject<dyn MTLCommandBuffer>>> {
        if self.path_intermediate_texture.is_none() {
            self.update_path_intermediate_textures(viewport_size)?;
        }
        crate::fast::layers::raster::rasterize_tiles(self, scene);
        let mut writer = InstanceBufferWriter::new(
            &self.device,
            &self.instance_buffer_pool,
            self.is_unified_memory,
        )?;
        let instance_bindings = write_instances(scene, &mut writer).with_context(|| {
            format!(
                "writing instance data: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces",
                scene.paths.len(),
                scene.shadows.len(),
                scene.quads.len(),
                scene.underlines.len(),
                scene.monochrome_sprites.len(),
                scene.polychrome_sprites.len(),
                scene.surfaces.len(),
            )
        })?;
        let mut surface_textures = Vec::new();
        // Koyo: what the frame's own scene asks of pictures is noted as its surfaces are drawn,
        // not what a layer's content, drawn into its tiles above, would.
        self.pictures.asked.clear();
        self.pictures.panes.clear();
        let command_buffer = self.draw_primitives_to_texture(
            scene,
            &instance_bindings,
            &mut writer,
            texture,
            viewport_size,
            &mut surface_textures,
        )?;
        // Koyo: the pictures the scene asks for are taken of the frame as it's drawn by now.
        self.take_pictures(texture, &command_buffer);

        let instance_buffer_pool = self.instance_buffer_pool.clone();
        // Koyo: the page surfaces this frame samples are free once it's done.
        let page_surfaces = mem::take(&mut self.page_surfaces_in_flight);
        let frame_resources = Cell::new(Some((writer.finish(), surface_textures, page_surfaces)));
        let block = RcBlock::new(
            move |_: ptr::NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                if let Some((instance_buffer, _surface_textures, _page_surfaces)) =
                    frame_resources.take()
                {
                    instance_buffer_pool.lock().release(instance_buffer);
                }
            },
        );
        // SAFETY: Metal copies the live block and invokes it once after this frame completes.
        // Its captures own the frame resources and use the pool mutex when returning the buffer.
        unsafe {
            command_buffer.addCompletedHandler(RcBlock::as_ptr(&block));
        }

        Ok(command_buffer)
    }

    /// Renders the scene to a texture and returns the pixel data as an RGBA image.
    /// This does not present the frame to screen - useful for visual testing
    /// where we want to capture what would be rendered without displaying it.
    ///
    /// Note: This requires a layer-backed renderer. For headless rendering,
    /// use `render_scene_to_image()` instead.
    #[cfg(any(test, feature = "test-support"))]
    pub fn render_to_image(&mut self, scene: &Scene) -> Result<RgbaImage> {
        let layer = self
            .layer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("render_to_image requires a layer-backed renderer"))?;
        let viewport_size = layer.drawableSize();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        // CAMetalDisplayLink owns the drawable, so render into a texture we own.
        self.render_offscreen(scene, viewport_size)
    }

    /// Renders a scene to an image without requiring a window or CAMetalLayer.
    ///
    /// This is the primary method for headless rendering. It creates an offscreen
    /// texture, renders the scene to it, and returns the pixel data as an RGBA image.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<RgbaImage> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene_to_image: {:?}", size);
        }

        // Headless callers do not have a Cocoa event-loop pool to release
        // autoreleased command buffers and render-pass descriptors.
        objc2::rc::autoreleasepool(|_| {
            self.update_path_intermediate_textures(size)?;
            self.render_offscreen(scene, size)
        })
    }

    /// Renders a scene into a new offscreen texture of `size` and reads it
    /// back, without presenting anything.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    fn render_offscreen(&mut self, scene: &Scene, size: Size<DevicePixels>) -> Result<RgbaImage> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_offscreen: {:?}", size);
        }

        let texture_descriptor = MTLTextureDescriptor::new();
        // SAFETY: The dimensions are positive and this descriptor creates a 2D texture.
        unsafe {
            texture_descriptor.setWidth(size.width.0 as usize);
            texture_descriptor.setHeight(size.height.0 as usize);
        }
        texture_descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        texture_descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        // Like the atlas, only Apple GPUs can create shared textures on macOS;
        // Intel Macs cannot, even with unified memory. iOS has no managed storage.
        let uses_shared_storage = cfg!(target_os = "ios") || self.is_apple_gpu;
        texture_descriptor.setStorageMode(if uses_shared_storage {
            MTLStorageMode::Shared
        } else {
            MTLStorageMode::Managed
        });
        let target_texture = self
            .device
            .newTextureWithDescriptor(&texture_descriptor)
            .context("allocating offscreen render target")?;

        let command_buffer = self.render_frame(scene, &target_texture, size)?;

        // Managed textures require an explicit blit synchronize before the CPU
        // can read back the rendered data. Without this, get_bytes returns
        // stale zeros.
        if !uses_shared_storage {
            let blit = command_buffer
                .blitCommandEncoder()
                .context("creating blit command encoder")?;
            blit.synchronizeResource(ProtocolObject::from_ref(&*target_texture));
            blit.endEncoding();
        }

        command_buffer.commit();
        command_buffer.waitUntilCompleted();

        read_texture_to_image(&target_texture)
    }

    /// Renders a scene to a reused offscreen texture without reading pixels
    /// back or blocking on GPU completion.
    ///
    /// This mirrors the CPU cost of presenting a frame to a window (scene
    /// encoding, instance buffer writes, command submission) and is used by
    /// headless benchmark rendering, where the produced pixels are never
    /// inspected.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> Result<()> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene: {:?}", size);
        }

        objc2::rc::autoreleasepool(|_| {
            self.update_path_intermediate_textures(size)?;

            let needs_new_target = self.headless_render_target.as_ref().is_none_or(|texture| {
                texture.width() != size.width.0 as usize
                    || texture.height() != size.height.0 as usize
            });
            if needs_new_target {
                let texture_descriptor = MTLTextureDescriptor::new();
                unsafe {
                    texture_descriptor.setWidth(size.width.0 as usize);
                    texture_descriptor.setHeight(size.height.0 as usize);
                }
                texture_descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
                texture_descriptor
                    .setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
                texture_descriptor.setStorageMode(MTLStorageMode::Private);
                self.headless_render_target = Some(
                    self.device
                        .newTextureWithDescriptor(&texture_descriptor)
                        .context("allocating headless render target")?,
                );
            }
            let target_texture = self
                .headless_render_target
                .clone()
                .context("retrieving headless render target")?;

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // Commit without waiting, mirroring presentation to a real window where
            // the CPU doesn't block on the GPU.
            command_buffer.commit();
            Ok(())
        })
    }

    fn draw_primitives_to_texture(
        &mut self,
        scene: &Scene,
        instance_bindings: &InstanceBindings,
        writer: &mut InstanceBufferWriter,
        texture: &ProtocolObject<dyn MTLTexture>,
        viewport_size: Size<DevicePixels>,
        surface_textures: &mut Vec<CFRetained<CVMetalTexture>>,
    ) -> Result<Retained<ProtocolObject<dyn MTLCommandBuffer>>> {
        if let Some(command_buffer) = crate::fast::paths::draw_primitives_to_texture(
            self,
            scene,
            instance_bindings,
            writer,
            texture,
            viewport_size,
            surface_textures,
        )? {
            return Ok(command_buffer);
        }
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue
            .commandBuffer()
            .context("creating command buffer")?;
        crate::fast::dynamic_textures::encode_uploads(&command_buffer);
        let alpha = if self.opaque { 1. } else { 0. };

        let mut command_encoder = new_command_encoder_for_texture(
            &command_buffer,
            texture,
            viewport_size,
            Some(MTLClearColor {
                red: 0.,
                green: 0.,
                blue: 0.,
                alpha,
            }),
        )?;

        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::Shadows(range) => {
                    self.draw_shadows(range, instance_bindings, viewport_size, &command_encoder)
                }
                PrimitiveBatch::Quads(range) => {
                    self.draw_quads(range, instance_bindings, viewport_size, &command_encoder)
                }
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range];
                    command_encoder.endEncoding();

                    let did_draw = self.draw_paths_to_intermediate(
                        paths,
                        writer,
                        viewport_size,
                        &command_buffer,
                    )?;

                    command_encoder = new_command_encoder_for_texture(
                        &command_buffer,
                        texture,
                        viewport_size,
                        None,
                    )?;

                    if did_draw {
                        if let Err(error) = self.draw_paths_from_intermediate(
                            paths,
                            writer,
                            viewport_size,
                            &command_encoder,
                        ) {
                            command_encoder.endEncoding();
                            return Err(error);
                        }
                    }
                }
                PrimitiveBatch::Underlines(range) => {
                    self.draw_underlines(range, instance_bindings, viewport_size, &command_encoder)
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => self
                    .draw_monochrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        &command_encoder,
                    ),
                PrimitiveBatch::PolychromeSprites { texture_id, range } => self
                    .draw_polychrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        &command_encoder,
                    ),
                PrimitiveBatch::Surfaces(range) => {
                    if let Err(error) = self.draw_surfaces(
                        &scene.surfaces[range.clone()],
                        range.start,
                        instance_bindings,
                        viewport_size,
                        &command_encoder,
                        surface_textures,
                    ) {
                        command_encoder.endEncoding();
                        return Err(error);
                    }
                }
                PrimitiveBatch::SubpixelSprites { .. } => unreachable!(),
            }
        }

        command_encoder.endEncoding();

        Ok(command_buffer)
    }

    pub(crate) fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    ) -> Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertex_instance_bindings = writer.write(&vertices)?;

        let render_pass_descriptor = MTLRenderPassDescriptor::new();
        // SAFETY: Index zero is within the render pass's color attachment array.
        let color_attachment = unsafe {
            render_pass_descriptor
                .colorAttachments()
                .objectAtIndexedSubscript(0)
        };
        color_attachment.setLoadAction(MTLLoadAction::Clear);
        color_attachment.setClearColor(MTLClearColor {
            red: 0.,
            green: 0.,
            blue: 0.,
            alpha: 0.,
        });

        if let Some(msaa_texture) = &self.path_intermediate_msaa_texture {
            color_attachment.setTexture(Some(msaa_texture));
            color_attachment.setResolveTexture(Some(intermediate_texture));
            color_attachment.setStoreAction(MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.setTexture(Some(intermediate_texture));
            color_attachment.setStoreAction(MTLStoreAction::Store);
        }

        let command_encoder = command_buffer
            .renderCommandEncoderWithDescriptor(&render_pass_descriptor)
            .context("creating path render command encoder")?;
        command_encoder.setRenderPipelineState(&self.paths_rasterization_pipeline_state);
        // SAFETY: The binding contains vertices.len() shader-compatible vertices at an aligned offset.
        // Metal retains the buffer, and writer.finish publishes CPU writes before submission.
        // The viewport bytes are copied from a live value into the matching shader slot.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&vertex_instance_bindings.buffer),
                vertex_instance_bindings.offset,
                PathRasterizationInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport_size).cast(),
                mem::size_of_val(&viewport_size),
                PathRasterizationInputIndex::ViewportSize as usize,
            );
            command_encoder.setFragmentBuffer_offset_atIndex(
                Some(&vertex_instance_bindings.buffer),
                vertex_instance_bindings.offset,
                PathRasterizationInputIndex::Vertices as usize,
            );
            command_encoder.drawPrimitives_vertexStart_vertexCount(
                MTLPrimitiveType::Triangle,
                0,
                vertices.len(),
            );
        }

        command_encoder.endEncoding();
        Ok(true)
    }

    pub(crate) fn draw_shadows(
        &self,
        shadows: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        if shadows.is_empty() {
            return;
        }

        command_encoder.setRenderPipelineState(&self.shadows_pipeline_state);
        // SAFETY: Six unit vertices and the scene's shadow range match the uploaded shader layouts and slots.
        // Metal retains the buffers, writer.finish publishes their writes, and viewport bytes are copied.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                ShadowInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&instance_bindings.shadows.buffer),
                instance_bindings.shadows.offset,
                ShadowInputIndex::Shadows as usize,
            );
            command_encoder.setFragmentBuffer_offset_atIndex(
                Some(&instance_bindings.shadows.buffer),
                instance_bindings.shadows.offset,
                ShadowInputIndex::Shadows as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport_size).cast(),
                mem::size_of_val(&viewport_size),
                ShadowInputIndex::ViewportSize as usize,
            );
            command_encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                MTLPrimitiveType::Triangle,
                0,
                6,
                shadows.len(),
                shadows.start,
            );
        }
    }

    pub(crate) fn draw_quads(
        &self,
        quads: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        if quads.is_empty() {
            return;
        }

        command_encoder.setRenderPipelineState(&self.quads_pipeline_state);
        // SAFETY: Six unit vertices and the scene's quad range match the uploaded shader layouts and slots.
        // Metal retains the buffers, writer.finish publishes their writes, and viewport bytes are copied.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                QuadInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&instance_bindings.quads.buffer),
                instance_bindings.quads.offset,
                QuadInputIndex::Quads as usize,
            );
            command_encoder.setFragmentBuffer_offset_atIndex(
                Some(&instance_bindings.quads.buffer),
                instance_bindings.quads.offset,
                QuadInputIndex::Quads as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport_size).cast(),
                mem::size_of_val(&viewport_size),
                QuadInputIndex::ViewportSize as usize,
            );
            command_encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                MTLPrimitiveType::Triangle,
                0,
                6,
                quads.len(),
                quads.start,
            );
        }
    }

    pub(crate) fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) -> Result<()> {
        let (Some(first_path), Some(last_path)) = (paths.first(), paths.last()) else {
            return Ok(());
        };
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        command_encoder.setRenderPipelineState(&self.path_sprites_pipeline_state);
        // SAFETY: The shader slots match the unit vertices, copied viewport, and retained path texture.
        // The preceding render pass writes this tracked texture before it is sampled.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                SpriteInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport_size).cast(),
                mem::size_of_val(&viewport_size),
                SpriteInputIndex::ViewportSize as usize,
            );
            command_encoder.setFragmentTexture_atIndex(
                Some(intermediate_texture),
                SpriteInputIndex::AtlasTexture as usize,
            );
        }

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if last_path.order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        let sprite_instance_bindings = writer.write(&sprites)?;
        // SAFETY: The draw uses six unit vertices and exactly the sprites written at this aligned binding.
        // Metal retains the buffer, and writer.finish publishes CPU writes before submission.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&sprite_instance_bindings.buffer),
                sprite_instance_bindings.offset,
                SpriteInputIndex::Sprites as usize,
            );
            command_encoder.drawPrimitives_vertexStart_vertexCount_instanceCount(
                MTLPrimitiveType::Triangle,
                0,
                6,
                sprites.len(),
            );
        }
        Ok(())
    }

    pub(crate) fn draw_underlines(
        &self,
        underlines: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        if underlines.is_empty() {
            return;
        }

        command_encoder.setRenderPipelineState(&self.underlines_pipeline_state);
        // SAFETY: Six unit vertices and the scene's underline range match the uploaded shader layouts and slots.
        // Metal retains the buffers, writer.finish publishes their writes, and viewport bytes are copied.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                UnderlineInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&instance_bindings.underlines.buffer),
                instance_bindings.underlines.offset,
                UnderlineInputIndex::Underlines as usize,
            );
            command_encoder.setFragmentBuffer_offset_atIndex(
                Some(&instance_bindings.underlines.buffer),
                instance_bindings.underlines.offset,
                UnderlineInputIndex::Underlines as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport_size).cast(),
                mem::size_of_val(&viewport_size),
                UnderlineInputIndex::ViewportSize as usize,
            );
            command_encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                MTLPrimitiveType::Triangle,
                0,
                6,
                underlines.len(),
                underlines.start,
            );
        }
    }

    pub(crate) fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        if sprites.is_empty() {
            return;
        }

        let texture = crate::fast::dynamic_textures::metal_texture(&self.device, texture_id)
            .or_else(|| self.sprite_atlas.metal_texture(texture_id));
        let Some(texture) = texture else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.setRenderPipelineState(&self.monochrome_sprites_pipeline_state);
        // SAFETY: The scene's sprite range, six unit vertices, and copied sizes match the shader layouts and slots.
        // Metal retains the buffers and texture, and writer.finish publishes the instance writes.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                SpriteInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&instance_bindings.monochrome_sprites.buffer),
                instance_bindings.monochrome_sprites.offset,
                SpriteInputIndex::Sprites as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport_size).cast(),
                mem::size_of_val(&viewport_size),
                SpriteInputIndex::ViewportSize as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&texture_size).cast(),
                mem::size_of_val(&texture_size),
                SpriteInputIndex::AtlasTextureSize as usize,
            );
            command_encoder.setFragmentBuffer_offset_atIndex(
                Some(&instance_bindings.monochrome_sprites.buffer),
                instance_bindings.monochrome_sprites.offset,
                SpriteInputIndex::Sprites as usize,
            );
            command_encoder.setFragmentTexture_atIndex(
                Some(&texture),
                SpriteInputIndex::AtlasTexture as usize,
            );
            command_encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                MTLPrimitiveType::Triangle,
                0,
                6,
                sprites.len(),
                sprites.start,
            );
        }
    }

    pub(crate) fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        if sprites.is_empty() {
            return;
        }
        if crate::fast::layers::composite::draw_tiles(
            self,
            texture_id,
            &sprites,
            instance_bindings,
            viewport_size,
            command_encoder,
        ) {
            return;
        }

        let texture = crate::fast::dynamic_textures::metal_texture(&self.device, texture_id)
            .or_else(|| self.sprite_atlas.metal_texture(texture_id));
        let Some(texture) = texture else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.setRenderPipelineState(&self.polychrome_sprites_pipeline_state);
        // SAFETY: The scene's sprite range, six unit vertices, and copied sizes match the shader layouts and slots.
        // Metal retains the buffers and texture, and writer.finish publishes the instance writes.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                SpriteInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&instance_bindings.polychrome_sprites.buffer),
                instance_bindings.polychrome_sprites.offset,
                SpriteInputIndex::Sprites as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport_size).cast(),
                mem::size_of_val(&viewport_size),
                SpriteInputIndex::ViewportSize as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&texture_size).cast(),
                mem::size_of_val(&texture_size),
                SpriteInputIndex::AtlasTextureSize as usize,
            );
            command_encoder.setFragmentBuffer_offset_atIndex(
                Some(&instance_bindings.polychrome_sprites.buffer),
                instance_bindings.polychrome_sprites.offset,
                SpriteInputIndex::Sprites as usize,
            );
            command_encoder.setFragmentTexture_atIndex(
                Some(&texture),
                SpriteInputIndex::AtlasTexture as usize,
            );
            command_encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                MTLPrimitiveType::Triangle,
                0,
                6,
                sprites.len(),
                sprites.start,
            );
        }
    }

    pub(crate) fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        first_surface: usize,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        surface_textures: &mut Vec<CFRetained<CVMetalTexture>>,
    ) -> Result<()> {
        if surfaces.is_empty() {
            return Ok(());
        }

        command_encoder.setRenderPipelineState(&self.surfaces_pipeline_state);
        // SAFETY: The unit vertices, SurfaceBounds instances, and copied viewport match the shader layouts and slots.
        // Metal retains the buffers, and writer.finish publishes CPU writes before submission.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                SurfaceInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&instance_bindings.surfaces.buffer),
                instance_bindings.surfaces.offset,
                SurfaceInputIndex::Surfaces as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&viewport_size).cast(),
                mem::size_of_val(&viewport_size),
                SurfaceInputIndex::ViewportSize as usize,
            );
        }

        for (index, surface) in surfaces.iter().enumerate() {
            let image_buffer = &surface.image_buffer;
            let texture_size = size(
                DevicePixels::from(CVPixelBufferGetWidth(image_buffer) as i32),
                DevicePixels::from(CVPixelBufferGetHeight(image_buffer) as i32),
            );

            let pixel_format = CVPixelBufferGetPixelFormatType(image_buffer);
            // Koyo: a BGRA surface, as a browser page's frame is, has a pipeline of its own.
            if pixel_format == kCVPixelFormatType_32BGRA {
                self.draw_page_surface(surface, texture_size, command_encoder);
                command_encoder.setRenderPipelineState(&self.surfaces_pipeline_state);
                continue;
            }
            anyhow::ensure!(
                pixel_format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                "unsupported surface pixel format: {pixel_format:#x}"
            );

            let plane_texture = |plane, format| -> Result<CFRetained<CVMetalTexture>> {
                let width = CVPixelBufferGetWidthOfPlane(image_buffer, plane);
                let height = CVPixelBufferGetHeightOfPlane(image_buffer, plane);
                let mut texture = ptr::null_mut();
                // SAFETY: The buffer has two planes matching these formats and dimensions, and the output pointer is writable.
                let status = unsafe {
                    CVMetalTextureCache::create_texture_from_image(
                        None,
                        &self.core_video_texture_cache,
                        image_buffer,
                        None,
                        format,
                        width,
                        height,
                        plane,
                        ptr::NonNull::from(&mut texture),
                    )
                };
                anyhow::ensure!(
                    status == kCVReturnSuccess,
                    "creating CoreVideo Metal texture for plane {plane}: status {status}"
                );
                let texture =
                    ptr::NonNull::new(texture).context("retrieving CoreVideo Metal texture")?;
                // SAFETY: The successful create call transfers ownership of this non-null texture.
                Ok(unsafe { CFRetained::from_raw(texture) })
            };
            let y_texture = plane_texture(0, MTLPixelFormat::R8Unorm)?;
            let cb_cr_texture = plane_texture(1, MTLPixelFormat::RG8Unorm)?;
            let y_metal_texture =
                CVMetalTextureGetTexture(&y_texture).context("retrieving luma Metal texture")?;
            let cb_cr_metal_texture = CVMetalTextureGetTexture(&cb_cr_texture)
                .context("retrieving chroma Metal texture")?;

            // SAFETY: The size is copied from a live value, and both planes match their shader texture slots.
            // The frame completion handler retains the CoreVideo wrappers until GPU reads finish.
            unsafe {
                command_encoder.setVertexBytes_length_atIndex(
                    ptr::NonNull::from(&texture_size).cast(),
                    mem::size_of_val(&texture_size),
                    SurfaceInputIndex::TextureSize as usize,
                );
                command_encoder.setFragmentTexture_atIndex(
                    Some(&y_metal_texture),
                    SurfaceInputIndex::YTexture as usize,
                );
                command_encoder.setFragmentTexture_atIndex(
                    Some(&cb_cr_metal_texture),
                    SurfaceInputIndex::CbCrTexture as usize,
                );
            }
            surface_textures.extend([y_texture, cb_cr_texture]);

            // SAFETY: Six unit vertices draw one instance within the uploaded scene's surface range.
            unsafe {
                command_encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                    MTLPrimitiveType::Triangle,
                    0,
                    6,
                    1,
                    first_surface + index,
                );
            }
        }
        Ok(())
    }
}

impl MetalRenderer {
    /// Koyo: draws a BGRA surface. With a picture (`set_surface_picture`), that top-left part
    /// of its texture goes over the surface's bounds and the quad fills the surface's content
    /// mask, the picture's edges reaching out to it; without one, the whole texture goes over
    /// the bounds, as far as the content mask shows them.
    fn draw_page_surface(
        &mut self,
        surface: &PaintSurface,
        texture_size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        let image_buffer = &surface.image_buffer;
        // Koyo: a surface a picture is taken into draws nothing, the frame is drawn into it;
        // nor does a pane, which only a picture shows. Both are noted for `take_pictures`.
        let picture = match surface_mark(image_buffer) {
            Some(Mark::Capture(request)) => {
                self.pictures.asked.push((surface.clone(), request));
                return;
            }
            Some(Mark::Pane(pane)) => {
                self.pictures.panes.push((surface.clone(), pane));
                return;
            }
            Some(Mark::Picture(picture)) => Some(picture),
            // Koyo: a chat's backdrop is a texture of the app's own, dithered and faded.
            Some(Mark::Backdrop(backdrop)) => {
                self.draw_backdrop(surface, backdrop, command_encoder);
                return;
            }
            None => None,
        };
        let mask = surface.content_mask.bounds;
        let quad = if picture.is_some() {
            mask
        } else {
            surface.bounds.intersect(&mask)
        };
        if quad.size.width.0 <= 0. || quad.size.height.0 <= 0. {
            return;
        }
        let texture_extent = [texture_size.width.0 as f32, texture_size.height.0 as f32];
        // Made over the surface itself, not through Core Video's texture cache, which would
        // keep the pixel buffer, so its surface in use, until the cache is next flushed.
        let usage = MTLTextureUsage::ShaderRead;
        let Some(texture) = page_surface_texture(&self.device, image_buffer, texture_size, usage)
        else {
            log::error!("a BGRA surface has no Metal texture");
            return;
        };
        let page = PageSurface {
            quad,
            mapping: surface.bounds,
            picture: picture.unwrap_or(texture_extent),
            texture_size: texture_extent,
        };
        self.draw_page(&page, &texture, command_encoder);
        // Kept until the frame is done.
        self.page_surfaces_in_flight
            .push((image_buffer.clone(), texture));
    }

    /// Koyo: draws `page` of `texture`, a BGRA surface's, with `command_encoder`, whose unit
    /// vertices and viewport are set.
    fn draw_page(
        &self,
        page: &PageSurface,
        texture: &ProtocolObject<dyn MTLTexture>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        command_encoder.setRenderPipelineState(&self.page_surfaces_pipeline_state);
        // SAFETY: `PageSurface` is laid out as the page shaders read it at this slot, Metal
        // copies the bytes before the calls return, and the texture matches the shader's slot.
        // Six unit vertices draw the one quad.
        unsafe {
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(page).cast(),
                mem::size_of_val(page),
                SurfaceInputIndex::Page as usize,
            );
            command_encoder.setFragmentBytes_length_atIndex(
                ptr::NonNull::from(page).cast(),
                mem::size_of_val(page),
                SurfaceInputIndex::Page as usize,
            );
            command_encoder
                .setFragmentTexture_atIndex(Some(texture), SurfaceInputIndex::YTexture as usize);
            command_encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6);
        }
    }

    /// Koyo: draws `backdrop`'s texture (`register_texture`) whole over `surface`'s bounds, as
    /// far as its content mask shows them, dithered, faint and fading down the content mask as
    /// `backdrop` says; nothing while no texture has its number. The command buffer keeps the
    /// texture until the GPU is done with it.
    fn draw_backdrop(
        &self,
        surface: &PaintSurface,
        backdrop: Backdrop,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        let Some(AppTexture(texture)) = TEXTURES.lock().get(&backdrop.texture).cloned() else {
            return;
        };
        let pane = surface.content_mask.bounds;
        let quad = surface.bounds.intersect(&pane);
        if quad.size.width.0 <= 0. || quad.size.height.0 <= 0. {
            return;
        }
        // Past the pane's bottom, so it never fades.
        let [fade_start, fade_end] = backdrop.fade.unwrap_or([2., 3.]);
        let params = BackdropSurface {
            quad,
            mapping: surface.bounds,
            pane,
            cell: backdrop.cell.max(1.),
            density: backdrop.density,
            opacity: backdrop.opacity.clamp(0., 1.),
            fade_start,
            fade_end,
        };
        command_encoder.setRenderPipelineState(&self.backdrop_pipeline_state);
        // SAFETY: `BackdropSurface` is laid out as the backdrop shaders read it at this slot,
        // Metal copies the bytes before the calls return, and the command buffer retains the
        // texture. Six unit vertices draw the one quad.
        unsafe {
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&params).cast(),
                mem::size_of_val(&params),
                SurfaceInputIndex::Page as usize,
            );
            command_encoder.setFragmentBytes_length_atIndex(
                ptr::NonNull::from(&params).cast(),
                mem::size_of_val(&params),
                SurfaceInputIndex::Page as usize,
            );
            command_encoder
                .setFragmentTexture_atIndex(Some(&texture), SurfaceInputIndex::YTexture as usize);
            command_encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6);
        }
    }
}

/// Koyo: what `backdrop_vertex` and `backdrop_fragment` take of a chat's backdrop.
#[repr(C)]
struct BackdropSurface {
    quad: Bounds<ScaledPixels>,
    /// The screen, which the texture covers whole.
    mapping: Bounds<ScaledPixels>,
    /// What the fade runs down: the surface's content mask, the chat's pane.
    pane: Bounds<ScaledPixels>,
    cell: f32,
    density: f32,
    opacity: f32,
    fade_start: f32,
    fade_end: f32,
}

/// Koyo: a Metal texture over the `IOSurface` a BGRA pixel buffer is made over, `size` of it,
/// to read or to draw into as `usage` says.
fn page_surface_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    image_buffer: &CVPixelBuffer,
    size: Size<DevicePixels>,
    usage: MTLTextureUsage,
) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    let surface = objc2_core_video::CVPixelBufferGetIOSurface(Some(image_buffer))?;
    // SAFETY: a 2D texture the size of the surface.
    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::BGRA8Unorm,
            size.width.0 as usize,
            size.height.0 as usize,
            false,
        )
    };
    descriptor.setUsage(usage);
    device.newTextureWithDescriptor_iosurface_plane(&descriptor, &surface, 0)
}

/// Koyo: what `page_surface_vertex` and `page_surface_fragment` take of a BGRA surface.
#[repr(C)]
struct PageSurface {
    quad: Bounds<ScaledPixels>,
    mapping: Bounds<ScaledPixels>,
    /// The size of the picture at the texture's top left, in texels.
    picture: [f32; 2],
    texture_size: [f32; 2],
}

thread_local! {
    /// Koyo: the attachment that says what a BGRA surface is beyond pixels to draw (`Mark`).
    static SURFACE_MARK: CFRetained<CFString> = CFString::from_static_str("KoyoSurfaceMark");
}

/// Koyo: what a BGRA surface painted in a scene is beyond pixels to draw. One attachment says
/// it, so drawing a surface reads it once.
#[derive(Clone, Copy)]
enum Mark {
    /// This much of it, at its top left, in texels, is its picture (`set_surface_picture`).
    Picture([f32; 2]),
    /// A picture is taken into it, for this request (`set_surface_capture`).
    Capture(u32),
    /// It stands for a native view in a picture (`set_surface_pane`).
    Pane(SurfacePane),
    /// It stands for a chat's backdrop, a texture of the app's own (`set_surface_backdrop`).
    Backdrop(Backdrop),
}

impl Mark {
    /// The mark as the attachment holds it: which it is, then what it says.
    fn words(self) -> Vec<u32> {
        match self {
            Mark::Picture(size) => iter::once(0).chain(size.map(f32::to_bits)).collect(),
            Mark::Capture(request) => vec![1, request],
            Mark::Pane(SurfacePane { radius, fill, edge }) => {
                let said = [
                    radius, fill.h, fill.s, fill.l, fill.a, edge.h, edge.s, edge.l, edge.a,
                ];
                iter::once(2).chain(said.map(f32::to_bits)).collect()
            }
            Mark::Backdrop(backdrop) => {
                let [start, end] = backdrop.fade.unwrap_or([f32::NAN; 2]);
                let id = backdrop.texture;
                let said = [
                    backdrop.cell,
                    backdrop.density,
                    backdrop.opacity,
                    start,
                    end,
                ];
                [3, id as u32, (id >> 32) as u32]
                    .into_iter()
                    .chain(said.map(f32::to_bits))
                    .collect()
            }
        }
    }

    /// The mark whose words, as `words` wrote them, `word` gives by their place.
    fn read(word: impl Fn(usize) -> Option<u32>) -> Option<Mark> {
        let part = |at: usize| word(at).map(f32::from_bits);
        let colour = |at: usize| {
            Some(Hsla {
                h: part(at)?,
                s: part(at + 1)?,
                l: part(at + 2)?,
                a: part(at + 3)?,
            })
        };
        match word(0)? {
            0 => Some(Mark::Picture([part(1)?, part(2)?])),
            1 => Some(Mark::Capture(word(1)?)),
            2 => Some(Mark::Pane(SurfacePane {
                radius: part(1)?,
                fill: colour(2)?,
                edge: colour(6)?,
            })),
            3 => {
                let (start, end) = (part(6)?, part(7)?);
                Some(Mark::Backdrop(Backdrop {
                    texture: u64::from(word(1)?) | (u64::from(word(2)?) << 32),
                    cell: part(3)?,
                    density: part(4)?,
                    opacity: part(5)?,
                    fade: (!start.is_nan()).then_some([start, end]),
                }))
            }
            _ => None,
        }
    }
}

/// Koyo: marks `buffer`, which keeps the mark for every scene that draws it after.
fn mark_surface(buffer: &CVPixelBuffer, mark: Mark) {
    let bytes: Vec<u8> = mark
        .words()
        .iter()
        .flat_map(|word| word.to_ne_bytes())
        .collect();
    let data = CFData::from_bytes(&bytes);
    // SAFETY: the attachment is a `CFData`, which is how `surface_mark` reads it.
    SURFACE_MARK.with(|key| unsafe {
        buffer.set_attachment(key, &data, CVAttachmentMode::ShouldNotPropagate)
    });
}

/// Koyo: how `buffer` is marked, if it is.
fn surface_mark(buffer: &CVPixelBuffer) -> Option<Mark> {
    // SAFETY: a null mode pointer is allowed.
    let value = SURFACE_MARK.with(|key| unsafe { buffer.attachment(key, ptr::null_mut()) })?;
    let data = value.downcast::<CFData>().ok()?;
    // SAFETY: the data is what `mark_surface` attached, which nothing writes to after.
    let bytes = unsafe { data.as_bytes_unchecked() };
    Mark::read(|at| {
        let word = bytes.get(at * 4..at * 4 + 4)?;
        Some(u32::from_ne_bytes(word.try_into().ok()?))
    })
}

/// Koyo: says that `size` of `buffer`, a BGRA surface, at its top left, is its picture.
/// Painted, the picture goes over the surface's bounds, and the rest of its content mask is
/// filled by the picture's edges reaching out, as a browser fills a window grown faster than
/// its page is laid out. The buffer keeps it for every scene that draws it after.
pub fn set_surface_picture(buffer: &CVPixelBuffer, size: Size<DevicePixels>) {
    let size = [size.width.0 as f32, size.height.0 as f32];
    mark_surface(buffer, Mark::Picture(size));
}

/// Koyo: how often a picture is taken again while what asks for it asks the same
/// (`set_surface_capture`).
const PICTURE_EVERY: Duration = Duration::from_millis(250);

/// Koyo: has a picture taken into `buffer`, a BGRA surface, of what the window draws under it.
/// Painted in a scene, the buffer draws nothing itself: once the frame is drawn, the frame's
/// pixels under the surface's bounds, as far as its content mask shows them, are shrunk to fill
/// the buffer's own surface, which another buffer over it then paints as a picture. One is
/// taken at most every [`PICTURE_EVERY`] while `request` stays the same, and with the next frame
/// when it changes. Only a surface painted in the frame's own scene has its picture taken, not
/// one inside a layer's content.
///
/// A window that composes native views draws in layers, each a frame of its own with a
/// renderer of its own (`fast/composition.rs`). The buffer is painted on the lowest, from the
/// root view, whose renderer takes the picture; the renderer of each layer over it
/// ([`MetalRenderer::draws_over`]) then draws its own part of the frame over that picture, so
/// the picture is of all GPUI draws there. The native views between the layers are in no
/// frame: in the picture a pane stands for one ([`set_surface_pane`]).
pub fn set_surface_capture(buffer: &CVPixelBuffer, request: u32) {
    mark_surface(buffer, Mark::Capture(request));
}

/// Koyo: how a pane shows in a picture (`set_surface_pane`): how round its corners are, in the
/// frame's pixels, its fill, and the line just inside its edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePane {
    pub radius: f32,
    pub fill: Hsla,
    pub edge: Hsla,
}

/// Koyo: marks `buffer`, a BGRA one of any size, as a pane: what stands, in a picture
/// (`set_surface_capture`), for a native view the window shows between its layers, glass say,
/// which no frame holds. Painted in a scene, it draws nothing. Painted on a layer over the
/// window's lowest, it's filled in a picture as `pane` says, over its bounds as far as its
/// content mask shows them, under what that layer draws.
pub fn set_surface_pane(buffer: &CVPixelBuffer, pane: SurfacePane) {
    mark_surface(buffer, Mark::Pane(pane));
}

/// Koyo: the compressed formats a texture of the app's may be in (`register_texture`): ASTC in
/// blocks of 4 × 4 or 8 × 8 pixels, which every Apple GPU reads as it samples. Its colours are
/// read as they are, as GPUI's atlas holds them, not taken out of sRGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFormat {
    Astc4x4,
    Astc8x8,
}

/// Koyo: the textures the app made (`register_texture`), by the number each was given.
static TEXTURES: std::sync::LazyLock<Mutex<std::collections::HashMap<u64, AppTexture>>> =
    std::sync::LazyLock::new(Mutex::default);

/// Koyo: a texture the app made, which any thread may make, let go of or draw.
#[derive(Clone)]
struct AppTexture(Retained<ProtocolObject<dyn MTLTexture>>);

// SAFETY: a Metal texture is documented as safe to use from any thread, and this one is
// written once, before it is shared, and only read after.
unsafe impl Send for AppTexture {}
unsafe impl Sync for AppTexture {}
static NEXT_TEXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Koyo: makes a texture `width` by `height` in `format`, of `blocks`, row by row of blocks from
/// its top left, on the system's GPU, and gives the number a surface marked for it draws it by
/// (`set_surface_backdrop`) until it's let go (`release_texture`). Any thread may make one. None
/// when the blocks aren't the texture's or the GPU can't hold it.
pub fn register_texture(
    format: TextureFormat,
    width: u32,
    height: u32,
    blocks: &[u8],
) -> Option<u64> {
    let (pixel_format, side) = match format {
        TextureFormat::Astc4x4 => (MTLPixelFormat::ASTC_4x4_LDR, 4),
        TextureFormat::Astc8x8 => (MTLPixelFormat::ASTC_8x8_LDR, 8),
    };
    let row = u64::from(width.div_ceil(side)) * 16;
    if width == 0 || height == 0 || blocks.len() as u64 != row * u64::from(height.div_ceil(side)) {
        return None;
    }
    let device = MTLCreateSystemDefaultDevice()?;
    if !device.supportsFamily(MTLGPUFamily::Apple2) {
        return None;
    }
    let descriptor = MTLTextureDescriptor::new();
    descriptor.setPixelFormat(pixel_format);
    // SAFETY: the dimensions are positive and this descriptor creates a 2D texture.
    unsafe {
        descriptor.setWidth(width as usize);
        descriptor.setHeight(height as usize);
    }
    descriptor.setUsage(MTLTextureUsage::ShaderRead);
    descriptor.setStorageMode(MTLStorageMode::Shared);
    let texture = device.newTextureWithDescriptor(&descriptor)?;
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: width as usize,
            height: height as usize,
            depth: 1,
        },
    };
    // SAFETY: `blocks` holds every row of blocks of the region, `row` bytes each, as checked
    // above, and Metal copies them before the call returns.
    unsafe {
        texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            region,
            0,
            ptr::NonNull::from(blocks).cast(),
            row as usize,
        );
    }
    let id = NEXT_TEXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    TEXTURES.lock().insert(id, AppTexture(texture));
    Some(id)
}

/// Koyo: lets texture `id` go (`register_texture`): a frame the GPU is still drawing keeps it
/// until it's done, and a surface marked for it draws nothing from the next.
pub fn release_texture(id: u64) {
    TEXTURES.lock().remove(&id);
}

/// Koyo: how a chat's backdrop is drawn (`set_surface_backdrop`): the screen's picture,
/// dithered in its own colours, faint, and fading out down the chat's pane, as Monodrive's
/// Dither effect looks (github.com/donnes/monodrive, MIT, © 2026 Donald Silveira).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Backdrop {
    /// The picture, a texture of the app's own (`register_texture`), which covers the
    /// surface's bounds whole.
    pub texture: u64,
    /// How many screen pixels a cell of the dither is a side, from the bounds' top left.
    pub cell: f32,
    /// How many cells are lit: 1 as Monodrive lights them, more for more.
    pub density: f32,
    /// How much of it shows over what's under it, from 0 to 1.
    pub opacity: f32,
    /// Where it starts fading out and where it's gone, as parts of the content mask's height
    /// from its top; none for no fade.
    pub fade: Option<[f32; 2]>,
}

/// Koyo: marks `buffer`, a BGRA one of any size, as standing for a chat's backdrop. Painted in
/// a scene, `backdrop`'s texture goes whole over the surface's bounds, as far as its content
/// mask shows them, with the rounded clips over it, as a page's frame does: in cells of
/// `backdrop.cell` pixels from the bounds' top left, each sampled at its middle and lit or not
/// by a 4 × 4 Bayer matrix of thresholds, a lit cell at its colour's full brightness and an
/// unlit one at 8 % of it, at `backdrop.opacity`, fading down the content mask. Nothing is
/// drawn while no texture has its number. The buffer keeps the mark for every scene that draws
/// it after.
pub fn set_surface_backdrop(buffer: &CVPixelBuffer, backdrop: Backdrop) {
    mark_surface(buffer, Mark::Backdrop(backdrop));
}

/// Koyo: what a renderer keeps of the pictures taken of its frames.
#[derive(Default)]
struct Pictures {
    /// The surfaces the scene being drawn has pictures taken into, with their requests, and
    /// its panes: noted as its surfaces are drawn.
    asked: Vec<(PaintSurface, u32)>,
    panes: Vec<(PaintSurface, SurfacePane)>,
    /// The request the last picture answered, and when: the lowest layer's.
    last: Option<(u32, Instant)>,
    /// For the renderer of a layer over the window's lowest (`draws_over`), how many pictures
    /// the lowest had taken when this one last drew its part over one. None for the lowest's.
    over: Option<u64>,
    /// What the renderers of the window's layers share.
    window: Rc<RefCell<WindowPictures>>,
}

/// Koyo: what the renderers of a window's layers share of its pictures (`draws_over`).
#[derive(Default)]
struct WindowPictures {
    /// The part of a frame last copied out, with the smaller copies of it a picture is shrunk
    /// from: kept while the next part is the same size, and used by each layer in its turn.
    source: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    /// Whether a picture has been asked of the window: every layer's frames are then to be
    /// read.
    asked: bool,
    /// The picture the lowest layer last took.
    taken: Option<Taken>,
}

/// Koyo: a picture the window's lowest layer took.
#[derive(Clone)]
struct Taken {
    /// The buffer it's in.
    into: CFRetained<CVPixelBuffer>,
    /// The part of the frame it's of.
    part: Bounds<ScaledPixels>,
    /// How many that layer has taken by now.
    count: u64,
}

impl MetalRenderer {
    /// Koyo: makes this the renderer of a layer over `base`'s in the same window. What it draws
    /// goes to the GPU in turn with what `base` draws, on the one queue, so its part of a
    /// picture is drawn after `base`'s; and it shares what `base` keeps of the window's
    /// pictures.
    pub(crate) fn draws_over(&mut self, base: &MetalRenderer) {
        self.command_queue = base.command_queue.clone();
        self.pictures.window = base.pictures.window.clone();
        self.pictures.over = Some(0);
    }

    /// Koyo: takes the pictures the scene just drawn into `frame` asks for
    /// (`set_surface_capture`), with what `command_buffer` holds so far; or, the renderer of a
    /// layer over the window's lowest, draws its part of the frame over the picture that one
    /// last took, once.
    fn take_pictures(
        &mut self,
        frame: &ProtocolObject<dyn MTLTexture>,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    ) {
        let Some(drawn_over) = self.pictures.over else {
            // Put back, to be emptied as the next frame is drawn, for the room it has.
            let asked = mem::take(&mut self.pictures.asked);
            for (surface, request) in &asked {
                self.take_picture(surface, *request, frame, command_buffer);
            }
            self.pictures.asked = asked;
            return;
        };
        let due = {
            let window = self.pictures.window.borrow();
            if !window.asked {
                return;
            }
            let taken = window.taken.as_ref();
            taken.filter(|taken| taken.count != drawn_over).cloned()
        };
        if !self.reads(frame) {
            return;
        }
        if let Some(taken) = due
            && self.shrink(taken.part, frame, &taken.into, true, command_buffer)
        {
            self.pictures.over = Some(taken.count);
        }
    }

    /// Koyo: whether `frame` can be read. A layer's frames can't until the layer is told they
    /// will be, which holds from its next: it's told here, when this one can't.
    fn reads(&self, frame: &ProtocolObject<dyn MTLTexture>) -> bool {
        if frame.isFramebufferOnly()
            && let Some(layer) = &self.layer
        {
            layer.setFramebufferOnly(false);
        }
        !frame.isFramebufferOnly()
    }

    /// Koyo: takes the picture `surface` asks for of `frame`; unless one was taken into its
    /// buffer for the same `request` less than [`PICTURE_EVERY`] ago.
    fn take_picture(
        &mut self,
        surface: &PaintSurface,
        request: u32,
        frame: &ProtocolObject<dyn MTLTexture>,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    ) {
        let into = &surface.image_buffer;
        let last = {
            let mut window = self.pictures.window.borrow_mut();
            window.asked = true;
            let same = |taken: &Taken| CFRetained::as_ptr(&taken.into) == CFRetained::as_ptr(into);
            window.taken.as_ref().map(|taken| (same(taken), taken.count))
        };
        if !self.reads(frame) {
            return;
        }
        let (same, count) = last.unwrap_or((false, 0));
        let now = Instant::now();
        if same
            && let Some((answered, at)) = self.pictures.last
            && answered == request
            && now.duration_since(at) < PICTURE_EVERY
        {
            return;
        }
        let part = surface.bounds.intersect(&surface.content_mask.bounds);
        if self.shrink(part, frame, into, false, command_buffer) {
            self.pictures.last = Some((request, now));
            self.pictures.window.borrow_mut().taken = Some(Taken {
                into: into.clone(),
                part,
                count: count + 1,
            });
        }
    }

    /// Koyo: copies the part of `frame` at `part` out, with smaller copies of it to shrink it
    /// from, and draws it shrunk over all of `into`'s own surface: in place of what was there,
    /// or, `over` it, blended into it, the scene's panes filled under it. Whether it was drawn:
    /// not if none of the part is in the frame.
    fn shrink(
        &mut self,
        part: Bounds<ScaledPixels>,
        frame: &ProtocolObject<dyn MTLTexture>,
        into: &CFRetained<CVPixelBuffer>,
        over: bool,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    ) -> bool {
        let left = part.origin.x.0.max(0.).floor();
        let top = part.origin.y.0.max(0.).floor();
        let right = (part.origin.x.0 + part.size.width.0).min(frame.width() as f32);
        let bottom = (part.origin.y.0 + part.size.height.0).min(frame.height() as f32);
        if right - left < 1. || bottom - top < 1. {
            return false;
        }
        let origin = MTLOrigin {
            x: left as usize,
            y: top as usize,
            z: 0,
        };
        let extent = MTLSize {
            width: (right - left) as usize,
            height: (bottom - top) as usize,
            depth: 1,
        };
        let size = size(
            DevicePixels::from(CVPixelBufferGetWidth(into) as i32),
            DevicePixels::from(CVPixelBufferGetHeight(into) as i32),
        );
        let usage = MTLTextureUsage::RenderTarget;
        let Some(picture) = page_surface_texture(&self.device, into, size, usage) else {
            log::error!("a surface a picture is taken into has no Metal texture");
            return false;
        };
        let Some(source) = self.picture_source(extent) else {
            log::error!("a picture has no texture to copy the frame into");
            return false;
        };
        let Some(blit) = command_buffer.blitCommandEncoder() else {
            log::error!("a picture has no encoder to copy the frame with");
            return false;
        };
        let start = MTLOrigin { x: 0, y: 0, z: 0 };
        // SAFETY: the part copied lies within `frame`, as clamped above, and `source` is its
        // size, a texture of the same format with room for its smaller copies.
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                frame, 0, 0, origin, extent, &source, 0, 0, start,
            );
        }
        blit.generateMipmapsForTexture(&source);
        blit.endEncoding();

        let clear = (!over).then_some(MTLClearColor {
            red: 0.,
            green: 0.,
            blue: 0.,
            alpha: 0.,
        });
        // `picture` is kept until the frame is done (`page_surfaces_in_flight`).
        let command_encoder =
            match new_command_encoder_for_texture(command_buffer, &picture, size, clear) {
                Ok(command_encoder) => command_encoder,
                Err(error) => {
                    log::error!("a picture has no encoder to draw it with: {error:#}");
                    return false;
                }
            };
        let copied = [extent.width as f32, extent.height as f32];
        if over {
            let scale = [
                size.width.0 as f32 / copied[0],
                size.height.0 as f32 / copied[1],
            ];
            self.fill_panes([left, top], scale, size, &command_encoder);
        }
        // The copy, all of it, over the whole picture: drawn as a page's frame is, which
        // samples the smaller copies as it shrinks.
        let whole = Bounds {
            origin: point(ScaledPixels(0.), ScaledPixels(0.)),
            size: gpui::size(
                ScaledPixels(size.width.0 as f32),
                ScaledPixels(size.height.0 as f32),
            ),
        };
        let page = PageSurface {
            quad: whole,
            mapping: whole,
            picture: copied,
            texture_size: copied,
        };
        // SAFETY: the unit vertices and the copied size match the page shaders' layouts and
        // slots; Metal retains the buffer and copies the bytes.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                SurfaceInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&size).cast(),
                mem::size_of_val(&size),
                SurfaceInputIndex::ViewportSize as usize,
            );
        }
        self.draw_page(&page, &source, &command_encoder);
        command_encoder.endEncoding();
        self.page_surfaces_in_flight.push((into.clone(), picture));
        true
    }

    /// Koyo: fills the scene's panes (`set_surface_pane`) in a picture `size` large that
    /// `command_encoder` draws into: each where it lies in the part of the frame the picture
    /// is of, which starts at `origin` and is `scale` as large in the picture, its edge a
    /// pixel of the picture wide so it shows at that size.
    fn fill_panes(
        &self,
        origin: [f32; 2],
        scale: [f32; 2],
        size: Size<DevicePixels>,
        command_encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    ) {
        if self.pictures.panes.is_empty() {
            return;
        }
        command_encoder.setRenderPipelineState(&self.quads_pipeline_state);
        // SAFETY: the unit vertices and the copied size match the quad shaders' layouts and
        // slots; Metal retains the buffer and copies the bytes.
        unsafe {
            command_encoder.setVertexBuffer_offset_atIndex(
                Some(&self.unit_vertices),
                0,
                QuadInputIndex::Vertices as usize,
            );
            command_encoder.setVertexBytes_length_atIndex(
                ptr::NonNull::from(&size).cast(),
                mem::size_of_val(&size),
                QuadInputIndex::ViewportSize as usize,
            );
        }
        let shrunk = |bounds: Bounds<ScaledPixels>| Bounds {
            origin: point(
                ScaledPixels((bounds.origin.x.0 - origin[0]) * scale[0]),
                ScaledPixels((bounds.origin.y.0 - origin[1]) * scale[1]),
            ),
            size: gpui::size(
                ScaledPixels(bounds.size.width.0 * scale[0]),
                ScaledPixels(bounds.size.height.0 * scale[1]),
            ),
        };
        for (surface, pane) in &self.pictures.panes {
            let bounds = shrunk(surface.bounds);
            let radius = (pane.radius * scale[0])
                .min(bounds.size.width.0 / 2.)
                .min(bounds.size.height.0 / 2.);
            let quad = Quad {
                bounds,
                content_mask: ContentMask {
                    bounds: shrunk(surface.content_mask.bounds),
                },
                background: pane.fill.into(),
                border_color: pane.edge,
                corner_radii: Corners::all(ScaledPixels(radius)),
                border_widths: Edges::all(ScaledPixels(1.)),
                ..Quad::default()
            };
            // SAFETY: one `Quad`, laid out as the quad shaders read their instances at this
            // slot, which Metal copies before the calls return. Six unit vertices draw it.
            unsafe {
                command_encoder.setVertexBytes_length_atIndex(
                    ptr::NonNull::from(&quad).cast(),
                    mem::size_of_val(&quad),
                    QuadInputIndex::Quads as usize,
                );
                command_encoder.setFragmentBytes_length_atIndex(
                    ptr::NonNull::from(&quad).cast(),
                    mem::size_of_val(&quad),
                    QuadInputIndex::Quads as usize,
                );
                command_encoder.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::Triangle,
                    0,
                    6,
                    1,
                );
            }
        }
    }

    /// Koyo: a texture `extent` in size to copy a part of a frame into, with room for the
    /// smaller copies of it: the last one the window's layers made, while it's that size.
    fn picture_source(&mut self, extent: MTLSize) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let mut window = self.pictures.window.borrow_mut();
        let kept = (window.source.as_ref())
            .filter(|source| (source.width(), source.height()) == (extent.width, extent.height));
        if let Some(source) = kept {
            return Some(source.clone());
        }
        let descriptor = MTLTextureDescriptor::new();
        // As many smaller copies as halving the longer side takes.
        let levels = (extent.width.max(extent.height) as f64).log2().ceil() as usize;
        // SAFETY: the dimensions are positive and this descriptor creates a 2D texture, which
        // has that many levels at most.
        unsafe {
            descriptor.setWidth(extent.width);
            descriptor.setHeight(extent.height);
            descriptor.setMipmapLevelCount(levels.max(1));
        }
        descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        descriptor.setStorageMode(MTLStorageMode::Private);
        let source = self.device.newTextureWithDescriptor(&descriptor)?;
        window.source = Some(source.clone());
        Some(source)
    }
}

pub(crate) fn new_command_encoder_for_texture(
    command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    texture: &ProtocolObject<dyn MTLTexture>,
    viewport_size: Size<DevicePixels>,
    clear_color: Option<MTLClearColor>,
) -> Result<Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>> {
    let render_pass_descriptor = MTLRenderPassDescriptor::new();
    // SAFETY: Index zero is within the render pass's color attachment array.
    let color_attachment = unsafe {
        render_pass_descriptor
            .colorAttachments()
            .objectAtIndexedSubscript(0)
    };
    color_attachment.setTexture(Some(texture));
    color_attachment.setStoreAction(MTLStoreAction::Store);
    if let Some(clear_color) = clear_color {
        color_attachment.setLoadAction(MTLLoadAction::Clear);
        color_attachment.setClearColor(clear_color);
    } else {
        color_attachment.setLoadAction(MTLLoadAction::Load);
    }

    let command_encoder = command_buffer
        .renderCommandEncoderWithDescriptor(&render_pass_descriptor)
        .context("creating render command encoder")?;
    command_encoder.setViewport(MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    crate::fast::rounded_clip::default_mask(&command_encoder);
    Ok(command_encoder)
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
fn read_texture_to_image(texture: &ProtocolObject<dyn MTLTexture>) -> Result<RgbaImage> {
    let width = texture.width() as u32;
    let height = texture.height() as u32;
    let bytes_per_row = width as usize * 4;
    let mut pixels = vec![0u8; height as usize * bytes_per_row];

    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: width as usize,
            height: height as usize,
            depth: 1,
        },
    };
    unsafe {
        texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            ptr::NonNull::from(pixels.as_mut_slice()).cast(),
            bytes_per_row,
            region,
            0,
        );
    }

    // Convert BGRA to RGBA (swap B and R channels)
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    RgbaImage::from_raw(width, height, pixels).context("creating RGBA image from pixel data")
}

fn build_pipeline_state(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: MTLPixelFormat,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
    let vertex_fn = library
        .newFunctionWithName(&NSString::from_str(vertex_fn_name))
        .with_context(|| format!("locating vertex function {vertex_fn_name}"))?;
    let fragment_fn = library
        .newFunctionWithName(&NSString::from_str(fragment_fn_name))
        .with_context(|| format!("locating fragment function {fragment_fn_name}"))?;

    let descriptor = MTLRenderPipelineDescriptor::new();
    descriptor.setLabel(Some(&NSString::from_str(label)));
    descriptor.setVertexFunction(Some(vertex_fn.as_ref()));
    descriptor.setFragmentFunction(Some(fragment_fn.as_ref()));
    // SAFETY: Index zero is within the pipeline's color attachment array.
    let color_attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    color_attachment.setPixelFormat(pixel_format);
    color_attachment.setBlendingEnabled(true);
    color_attachment.setRgbBlendOperation(MTLBlendOperation::Add);
    color_attachment.setAlphaBlendOperation(MTLBlendOperation::Add);
    color_attachment.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
    color_attachment.setSourceAlphaBlendFactor(MTLBlendFactor::One);
    color_attachment.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);

    device
        .newRenderPipelineStateWithDescriptor_error(&descriptor)
        .with_context(|| format!("creating {label} render pipeline"))
}

fn build_path_sprite_pipeline_state(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: MTLPixelFormat,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
    let vertex_fn = library
        .newFunctionWithName(&NSString::from_str(vertex_fn_name))
        .with_context(|| format!("locating vertex function {vertex_fn_name}"))?;
    let fragment_fn = library
        .newFunctionWithName(&NSString::from_str(fragment_fn_name))
        .with_context(|| format!("locating fragment function {fragment_fn_name}"))?;

    let descriptor = MTLRenderPipelineDescriptor::new();
    descriptor.setLabel(Some(&NSString::from_str(label)));
    descriptor.setVertexFunction(Some(vertex_fn.as_ref()));
    descriptor.setFragmentFunction(Some(fragment_fn.as_ref()));
    // SAFETY: Index zero is within the pipeline's color attachment array.
    let color_attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    color_attachment.setPixelFormat(pixel_format);
    color_attachment.setBlendingEnabled(true);
    color_attachment.setRgbBlendOperation(MTLBlendOperation::Add);
    color_attachment.setAlphaBlendOperation(MTLBlendOperation::Add);
    color_attachment.setSourceRGBBlendFactor(MTLBlendFactor::One);
    color_attachment.setSourceAlphaBlendFactor(MTLBlendFactor::One);
    color_attachment.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);

    device
        .newRenderPipelineStateWithDescriptor_error(&descriptor)
        .with_context(|| format!("creating {label} render pipeline"))
}

fn build_path_rasterization_pipeline_state(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: MTLPixelFormat,
    path_sample_count: u32,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
    let vertex_fn = library
        .newFunctionWithName(&NSString::from_str(vertex_fn_name))
        .with_context(|| format!("locating vertex function {vertex_fn_name}"))?;
    let fragment_fn = library
        .newFunctionWithName(&NSString::from_str(fragment_fn_name))
        .with_context(|| format!("locating fragment function {fragment_fn_name}"))?;

    let descriptor = MTLRenderPipelineDescriptor::new();
    descriptor.setLabel(Some(&NSString::from_str(label)));
    descriptor.setVertexFunction(Some(vertex_fn.as_ref()));
    descriptor.setFragmentFunction(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.setRasterSampleCount(path_sample_count as _);
        descriptor.setAlphaToCoverageEnabled(false);
    }
    // SAFETY: Index zero is within the pipeline's color attachment array.
    let color_attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    color_attachment.setPixelFormat(pixel_format);
    color_attachment.setBlendingEnabled(true);
    color_attachment.setRgbBlendOperation(MTLBlendOperation::Add);
    color_attachment.setAlphaBlendOperation(MTLBlendOperation::Add);
    color_attachment.setSourceRGBBlendFactor(MTLBlendFactor::One);
    color_attachment.setSourceAlphaBlendFactor(MTLBlendFactor::One);
    color_attachment.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);

    device
        .newRenderPipelineStateWithDescriptor_error(&descriptor)
        .with_context(|| format!("creating {label} render pipeline"))
}

#[derive(Clone)]
pub(crate) struct InstanceBinding {
    pub(crate) buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub(crate) offset: usize,
}

pub(crate) struct InstanceBindings {
    pub(crate) quads: InstanceBinding,
    pub(crate) shadows: InstanceBinding,
    pub(crate) underlines: InstanceBinding,
    pub(crate) monochrome_sprites: InstanceBinding,
    pub(crate) polychrome_sprites: InstanceBinding,
    pub(crate) surfaces: InstanceBinding,
}

pub(crate) fn write_instances(scene: &Scene, writer: &mut InstanceBufferWriter) -> Result<InstanceBindings> {
    Ok(InstanceBindings {
        quads: writer.write(&scene.quads)?,
        shadows: writer.write(&scene.shadows)?,
        underlines: writer.write(&scene.underlines)?,
        monochrome_sprites: writer.write(&scene.monochrome_sprites)?,
        polychrome_sprites: writer.write(&scene.polychrome_sprites)?,
        surfaces: writer.write_iter(scene.surfaces.iter().map(|surface| SurfaceBounds {
            bounds: surface.bounds,
            content_mask: surface.content_mask,
        }))?,
    })
}

pub(crate) struct InstanceBufferWriter {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    pool: Arc<Mutex<InstanceBufferPool>>,
    unified_memory: bool,
    filled: Vec<(InstanceBuffer, usize)>,
    current: InstanceBuffer,
    offset: usize,
}

impl InstanceBufferWriter {
    pub(crate) fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        pool: &Arc<Mutex<InstanceBufferPool>>,
        unified_memory: bool,
    ) -> Result<Self> {
        let current = pool.lock().acquire(device, unified_memory)?;
        Ok(Self {
            device: device.retain(),
            pool: pool.clone(),
            unified_memory,
            filled: Vec::new(),
            current,
            offset: 0,
        })
    }

    fn allocate<T>(&mut self, count: usize) -> Result<(InstanceBinding, &mut [MaybeUninit<T>])> {
        let size = mem::size_of::<T>() * count;
        let mut offset = self.offset.next_multiple_of(INSTANCE_BUFFER_ALIGNMENT);
        if offset + size > self.current.size {
            self.grow(size)?;
            offset = 0;
        }
        self.offset = offset + size;

        let binding = InstanceBinding {
            buffer: self.current.metal_buffer.clone(),
            offset,
        };
        // SAFETY: The aligned reservation lies within this frame's exclusively owned buffer and does not overlap prior writes.
        let values = unsafe {
            let start = self
                .current
                .metal_buffer
                .contents()
                .as_ptr()
                .cast::<u8>()
                .add(offset);
            slice::from_raw_parts_mut(start.cast::<MaybeUninit<T>>(), count)
        };
        Ok((binding, values))
    }

    pub(crate) fn write<T>(&mut self, values: &[T]) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        // SAFETY: The disjoint source and destination hold the same number of T values, and the destination stays MaybeUninit.
        unsafe {
            ptr::copy_nonoverlapping(
                values.as_ptr(),
                destination.as_mut_ptr().cast::<T>(),
                values.len(),
            );
        }
        Ok(binding)
    }

    pub(crate) fn write_iter<T>(
        &mut self,
        values: impl ExactSizeIterator<Item = T>,
    ) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        for (slot, value) in destination.iter_mut().zip(values) {
            slot.write(value);
        }
        Ok(binding)
    }

    fn grow(&mut self, required: usize) -> Result<()> {
        let mut pool = self.pool.lock();
        let buffer_size = (pool.buffer_size * 2)
            .max(required.next_power_of_two())
            .min(MAX_INSTANCE_BUFFER_SIZE);
        anyhow::ensure!(
            buffer_size >= required,
            "instance buffer needs {required} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE}"
        );
        anyhow::ensure!(
            buffer_size > self.current.size,
            "frame instance data exceeds the {MAX_INSTANCE_BUFFER_SIZE}-byte maximum"
        );
        if buffer_size != pool.buffer_size {
            log::info!("increased instance buffer size to {buffer_size}");
            pool.reset(buffer_size);
        }
        let buffer = pool.acquire(&self.device, self.unified_memory)?;
        drop(pool);

        let filled = mem::replace(&mut self.current, buffer);
        self.filled.push((filled, self.offset));
        self.offset = 0;
        Ok(())
    }

    pub(crate) fn finish(self) -> InstanceBuffer {
        let Self {
            unified_memory,
            filled,
            current,
            offset,
            ..
        } = self;

        if !unified_memory {
            for (buffer, written) in &filled {
                if *written == 0 {
                    continue;
                }
                buffer.metal_buffer.didModifyRange(NSRange {
                    location: 0,
                    length: *written,
                });
            }
            if offset > 0 {
                current.metal_buffer.didModifyRange(NSRange {
                    location: 0,
                    length: offset,
                });
            }
        }

        // Metal retains encoded resources until the command buffer completes.
        // Only the final, largest buffer is worth keeping in the pool.
        drop(filled);
        current
    }
}

#[repr(C)]
enum ShadowInputIndex {
    Vertices = 0,
    Shadows = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum QuadInputIndex {
    Vertices = 0,
    Quads = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum UnderlineInputIndex {
    Vertices = 0,
    Underlines = 1,
    ViewportSize = 2,
}

#[repr(C)]
pub(crate) enum SpriteInputIndex {
    Vertices = 0,
    Sprites = 1,
    ViewportSize = 2,
    AtlasTextureSize = 3,
    AtlasTexture = 4,
}

#[repr(C)]
enum SurfaceInputIndex {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
    /// Koyo: a BGRA surface's `PageSurface`.
    Page = 6,
}

#[repr(C)]
pub(crate) enum PathRasterizationInputIndex {
    Vertices = 0,
    ViewportSize = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PathSprite {
    pub bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SurfaceBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
pub struct MetalHeadlessRenderer {
    renderer: MetalRenderer,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl MetalHeadlessRenderer {
    pub fn new() -> Result<Self> {
        let instance_buffer_pool = Arc::new(Mutex::new(InstanceBufferPool::default()));
        let renderer = MetalRenderer::new_headless(instance_buffer_pool)?;
        Ok(Self { renderer })
    }
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl gpui::PlatformHeadlessRenderer for MetalHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.renderer.render_scene_to_image(scene, size)
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.renderer.render_scene(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
        self.renderer.sprite_atlas().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_foundation::{CFDictionary, CFType};
    use objc2_core_video::{
        CVPixelBufferCreate, CVPixelBufferGetBaseAddressOfPlane,
        CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVPixelBufferIOSurfacePropertiesKey,
    };
    use objc2_metal::MTLCommandBufferStatus;

    #[test]
    fn path_intermediate_textures_are_reused_until_resize() -> Result<()> {
        let mut renderer = MetalRenderer::new_headless(Default::default())?;
        let initial_size = size(DevicePixels(64), DevicePixels(64));
        renderer.update_path_intermediate_textures(initial_size)?;
        let initial_texture = renderer
            .path_intermediate_texture
            .clone()
            .context("missing path intermediate texture")?;
        let initial_msaa_texture = renderer.path_intermediate_msaa_texture.clone();

        renderer.update_path_intermediate_textures(initial_size)?;
        assert!(ptr::eq(
            &*initial_texture,
            renderer
                .path_intermediate_texture
                .as_deref()
                .context("missing reused texture")?,
        ));
        assert_eq!(
            initial_msaa_texture.as_ref().map(Retained::as_ptr),
            renderer
                .path_intermediate_msaa_texture
                .as_ref()
                .map(Retained::as_ptr),
        );

        renderer.update_path_intermediate_textures(size(DevicePixels(128), DevicePixels(64)))?;
        let resized_texture = renderer
            .path_intermediate_texture
            .as_deref()
            .context("missing resized texture")?;
        assert_eq!(
            (resized_texture.width(), resized_texture.height()),
            (128, 64)
        );
        assert!(!ptr::eq(&*initial_texture, resized_texture));
        if let Some(texture) = &renderer.path_intermediate_msaa_texture {
            assert_eq!((texture.width(), texture.height()), (128, 64));
        }

        for (width, height) in [(0, 64), (64, 0), (-1, 64), (64, -1)] {
            renderer.update_path_intermediate_textures(size(
                DevicePixels(width),
                DevicePixels(height),
            ))?;
            assert!(renderer.path_intermediate_texture.is_none());
            assert!(renderer.path_intermediate_msaa_texture.is_none());
            renderer.update_path_intermediate_textures(initial_size)?;
            assert!(renderer.path_intermediate_texture.is_some());
        }
        Ok(())
    }

    #[test]
    fn render_to_image_rejects_zero_sized_layer() -> Result<()> {
        let mut renderer = MetalRenderer::new(Default::default(), false, false)?;
        for (width, height) in [(0, 0), (0, 64), (64, 0)] {
            renderer.update_drawable_size(size(DevicePixels(width), DevicePixels(height)));
            assert!(renderer.render_to_image(&Scene::default()).is_err());
        }
        Ok(())
    }

    #[test]
    fn display_link_mode_consumes_and_clears_drawables() -> Result<()> {
        let mut renderer = MetalRenderer::new(Default::default(), false, false)?;
        renderer.update_drawable_size(size(DevicePixels(64), DevicePixels(64)));
        let drawable = renderer
            .layer()
            .and_then(|layer| layer.nextDrawable())
            .context("window renderer must supply a drawable")?;

        renderer.set_display_link_attached(true);
        assert!(matches!(
            renderer.display_link_mode,
            DisplayLinkMode::Attached { drawable: None }
        ));
        assert!(
            renderer
                .set_frame_drawable(Some(drawable.clone()))
                .is_none()
        );
        assert!(matches!(
            renderer.display_link_mode,
            DisplayLinkMode::Attached { drawable: Some(_) }
        ));
        renderer.draw(&Scene::default());
        assert!(matches!(
            renderer.display_link_mode,
            DisplayLinkMode::Attached { drawable: None }
        ));
        // A frame that drew consumed its drawable; one that didn't hands it back.
        assert!(
            renderer
                .set_frame_drawable(Some(drawable.clone()))
                .is_none()
        );
        assert!(renderer.set_frame_drawable(None).is_some());

        renderer.set_frame_drawable(Some(drawable));
        renderer.set_display_link_attached(false);
        assert!(matches!(
            renderer.display_link_mode,
            DisplayLinkMode::Detached
        ));
        Ok(())
    }

    #[test]
    fn render_core_video_surface() {
        let surface_properties_key = unsafe { kCVPixelBufferIOSurfacePropertiesKey };
        let attributes = CFDictionary::from_slices(
            &[surface_properties_key],
            &[&*CFDictionary::<CFType, CFType>::empty()],
        );
        let mut image_buffer = ptr::null_mut();
        assert_eq!(
            unsafe {
                CVPixelBufferCreate(
                    None,
                    16,
                    16,
                    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                    Some(attributes.as_opaque()),
                    ptr::NonNull::from(&mut image_buffer),
                )
            },
            kCVReturnSuccess
        );
        let image_buffer = ptr::NonNull::new(image_buffer).unwrap();
        let image_buffer = unsafe { CFRetained::from_raw(image_buffer) };
        unsafe {
            assert_eq!(
                CVPixelBufferLockBaseAddress(&image_buffer, CVPixelBufferLockFlags::empty()),
                kCVReturnSuccess
            );
            for plane in 0..2 {
                let bytes = CVPixelBufferGetBaseAddressOfPlane(&image_buffer, plane).cast::<u8>();
                let length = CVPixelBufferGetBytesPerRowOfPlane(&image_buffer, plane)
                    * CVPixelBufferGetHeightOfPlane(&image_buffer, plane);
                slice::from_raw_parts_mut(bytes, length).fill(128);
            }
            assert_eq!(
                CVPixelBufferUnlockBaseAddress(&image_buffer, CVPixelBufferLockFlags::empty()),
                kCVReturnSuccess
            );
        }

        let bounds = Bounds {
            origin: Point::default(),
            size: size(ScaledPixels(16.0), ScaledPixels(16.0)),
        };
        let mut scene = Scene::default();
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            image_buffer: image_buffer.clone(),
        });
        drop(image_buffer);
        scene.finish();

        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())))
                .unwrap();
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        for image::Rgba([red, green, blue, alpha]) in image.pixels() {
            assert!(red.abs_diff(128) <= 2);
            assert!(green.abs_diff(128) <= 2);
            assert!(blue.abs_diff(128) <= 2);
            assert_eq!(*alpha, 255);
        }

        let descriptor = MTLTextureDescriptor::new();
        unsafe {
            descriptor.setWidth(16);
            descriptor.setHeight(16);
        }
        descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        descriptor.setUsage(MTLTextureUsage::RenderTarget);
        let uses_shared_storage = cfg!(target_os = "ios") || renderer.is_apple_gpu;
        descriptor.setStorageMode(if uses_shared_storage {
            MTLStorageMode::Shared
        } else {
            MTLStorageMode::Managed
        });
        let target_texture = renderer
            .device
            .newTextureWithDescriptor(&descriptor)
            .unwrap();
        let command_buffer = renderer
            .render_frame(
                &scene,
                &target_texture,
                size(DevicePixels(16), DevicePixels(16)),
            )
            .unwrap();
        if !uses_shared_storage {
            let blit = command_buffer.blitCommandEncoder().unwrap();
            blit.synchronizeResource(ProtocolObject::from_ref(&*target_texture));
            blit.endEncoding();
        }
        drop(scene);
        drop(renderer);
        command_buffer.commit();
        command_buffer.waitUntilCompleted();
        assert_eq!(command_buffer.status(), MTLCommandBufferStatus::Completed);
        assert_eq!(read_texture_to_image(&target_texture).unwrap(), image);
    }
}

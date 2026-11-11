//! The GPU's part in the ring (`docs/devices.md` 3.1): textures over `IOSurface`s, and the copy of
//! a frame into a slot, a blit when the picture is upright as it comes, a small kernel that turns
//! it when the device is turned (the simulator draws a turned screen turned inside the same
//! surface). The copy is committed and not waited for.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{
    MTLBlitCommandEncoder, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLComputeCommandEncoder, MTLComputePipelineState, MTLCreateSystemDefaultDevice, MTLDevice,
    MTLLibrary, MTLPixelFormat, MTLSize, MTLTexture, MTLTextureDescriptor, MTLTextureUsage,
};

/// The kernel that turns: the slot's pixel at `p` is the source's at the place a turn of
/// `quarters` takes it from (`places.rs` maps touches the same way).
const TURN: &str = r"
#include <metal_stdlib>
using namespace metal;
kernel void turn(texture2d<half, access::read> source [[texture(0)]],
                 texture2d<half, access::write> slot [[texture(1)]],
                 constant uint &quarters [[buffer(0)]],
                 uint2 p [[thread_position_in_grid]]) {
    uint w = slot.get_width(), h = slot.get_height();
    if (p.x >= w || p.y >= h) return;
    uint2 s;
    if (quarters == 1) s = uint2(p.y, w - 1 - p.x);
    else if (quarters == 2) s = uint2(w - 1 - p.x, h - 1 - p.y);
    else if (quarters == 3) s = uint2(h - 1 - p.y, p.x);
    else s = p;
    slot.write(source.read(s), p);
}
";

pub type Texture = Retained<ProtocolObject<dyn MTLTexture>>;

pub struct Gpu {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    turn: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
}

// SAFETY: a Metal device, its command queue and a pipeline state are safe to use from any
// thread (Apple's Metal Programming Guide, "Thread Safety"); textures are made and encoded
// under the ring's lock.
unsafe impl Send for Gpu {}
// SAFETY: as above.
unsafe impl Sync for Gpu {}

impl Gpu {
    pub fn new() -> Result<Gpu, String> {
        let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        let queue = device.newCommandQueue().ok_or("no Metal command queue")?;
        let library = device
            .newLibraryWithSource_options_error(&NSString::from_str(TURN), None)
            .map_err(|error| format!("the turn kernel didn't compile: {error}"))?;
        let function = library
            .newFunctionWithName(&NSString::from_str("turn"))
            .ok_or("no turn function")?;
        let turn = device
            .newComputePipelineStateWithFunction_error(&function)
            .map_err(|error| format!("no turn pipeline: {error}"))?;
        Ok(Gpu {
            device,
            queue,
            turn,
        })
    }

    /// A BGRA texture over `surface`, for reading and writing.
    pub fn texture(&self, surface: &IOSurfaceRef) -> Option<Texture> {
        // SAFETY: a descriptor made for a 2D BGRA texture of the surface's size, its usage set
        // before it's used; the surface outlives the texture (both kept in one slot).
        unsafe {
            let descriptor =
                MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                    MTLPixelFormat::BGRA8Unorm,
                    surface.width(),
                    surface.height(),
                    false,
                );
            descriptor.setUsage(
                MTLTextureUsage::ShaderRead
                    | MTLTextureUsage::ShaderWrite
                    | MTLTextureUsage::RenderTarget,
            );
            self.device
                .newTextureWithDescriptor_iosurface_plane(&descriptor, surface, 0)
        }
    }

    /// Copies `source` into `slot`, turned by `quarters`, and calls `done` once the GPU has.
    /// Returns at once.
    pub fn copy(
        &self,
        source: &Texture,
        slot: &Texture,
        quarters: u8,
        done: impl Fn() + Send + 'static,
    ) -> Result<(), String> {
        let buffer = self.queue.commandBuffer().ok_or("no command buffer")?;
        if quarters == 0 {
            let blit = buffer.blitCommandEncoder().ok_or("no blit encoder")?;
            // SAFETY: two BGRA textures of one size.
            unsafe { blit.copyFromTexture_toTexture(source, slot) };
            blit.endEncoding();
        } else {
            let compute = buffer.computeCommandEncoder().ok_or("no compute encoder")?;
            compute.setComputePipelineState(&self.turn);
            let quarters = u32::from(quarters);
            // SAFETY: the kernel's two textures and its one constant, as it declares them; the
            // constant's bytes are copied when set.
            unsafe {
                compute.setTexture_atIndex(Some(source), 0);
                compute.setTexture_atIndex(Some(slot), 1);
                compute.setBytes_length_atIndex(NonNull::from(&quarters).cast(), 4, 0);
            }
            let grid = MTLSize {
                width: slot.width(),
                height: slot.height(),
                depth: 1,
            };
            let group = MTLSize {
                width: 16,
                height: 16,
                depth: 1,
            };
            compute.dispatchThreads_threadsPerThreadgroup(grid, group);
            compute.endEncoding();
        }
        let handler = RcBlock::new(move |_: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| done());
        // SAFETY: Metal copies the block and calls it once, on a thread of its own.
        unsafe { buffer.addCompletedHandler(RcBlock::as_ptr(&handler)) };
        buffer.commit();
        Ok(())
    }
}

//! Giving a refused message back: what it brought is released, as its header and descriptors
//! say.

use super::{Header, MACH_MSG_PORT_DESCRIPTOR, MACH_MSGH_BITS_COMPLEX, Received};

pub(super) const MACH_MSG_OOL_DESCRIPTOR: u8 = 1;
pub(super) const MACH_MSG_OOL_PORTS_DESCRIPTOR: u8 = 2;
pub(super) const MACH_MSG_OOL_VOLATILE_DESCRIPTOR: u8 = 3;
const MACH_MSG_GUARDED_PORT_DESCRIPTOR: u8 = 4;

unsafe extern "C" {
    fn mach_msg_destroy(msg: *mut Header);
}

/// Releases the rights and memory the message in `buffer` brought.
///
/// # Safety
/// `buffer` holds a message as the kernel copies one out to this process, whole or in part:
/// each right and each piece of memory it names came with it, and what the kernel couldn't
/// copy out reads as nothing there (a port as null or dead, memory as address 0 and size 0, a
/// port array as address 0).
pub(super) unsafe fn give_back(buffer: &mut Received) {
    let Header { bits, size, .. } = buffer.message.header;
    if bits & MACH_MSGH_BITS_COMPLEX != 0 {
        let count = buffer.message.descriptor_count;
        let start = size_of::<Header>() + size_of::<u32>();
        let message = bytes(buffer);
        let end = message.len().min(size as usize);
        if let Some(descriptors) = message.get_mut(start..end) {
            ready_for_destroy(descriptors, count);
        }
    }
    // SAFETY: a message as the kernel copies one out, as the caller promises, which destroying
    // reads as its header and descriptors say: what came with it is released once, and what
    // didn't come reads as nothing, a port array never placed here as count 0 once
    // `ready_for_destroy` has seen to it.
    unsafe { mach_msg_destroy(&raw mut buffer.message.header) };
}

/// `buffer`'s bytes, as the kernel writes a message there.
pub(super) fn bytes(buffer: &mut Received) -> &mut [u8] {
    // SAFETY: `Received` is integers alone, packed with no padding, so every byte of it is
    // initialized and any bytes written there are a `Received`; the slice borrows it whole.
    unsafe {
        std::slice::from_raw_parts_mut(
            std::ptr::from_mut(buffer).cast::<u8>(),
            size_of::<Received>(),
        )
    }
}

/// Rewrites the first `count` of `descriptors` so that `mach_msg_destroy` releases all they
/// brought and reads nothing that didn't come.
///
/// Destroying frees memory only where the descriptor's deallocate byte is set, and never
/// volatile memory, but the kernel sets that byte only for memory sent by virtual copy
/// (`ipc_kmsg_deflate_memory_descriptor`), though it maps every piece of memory it copies out:
/// each memory descriptor becomes a plain one with the byte set. Destroying still does the
/// freeing, rather than this walk, so that nothing is freed twice; memory the kernel couldn't
/// map is at size 0, which destroying passes over.
///
/// A port array the kernel couldn't place in this process it leaves at address 0 with the
/// count it was sent with (`ipc_kmsg_copyout_ool_ports_descriptor`), and destroying would read
/// that many names from address 0: its count becomes 0.
///
/// The descriptors are as 64-bit userland lays them out, each with its deallocate byte ninth
/// and its type twelfth; one not whole in `descriptors` ends the walk.
fn ready_for_destroy(descriptors: &mut [u8], count: u32) {
    let mut at = 0;
    for _ in 0..count {
        let Some(&kind) = descriptors.get(at + 11) else {
            return;
        };
        let length = match kind {
            MACH_MSG_PORT_DESCRIPTOR => 12,
            MACH_MSG_OOL_DESCRIPTOR
            | MACH_MSG_OOL_PORTS_DESCRIPTOR
            | MACH_MSG_OOL_VOLATILE_DESCRIPTOR
            | MACH_MSG_GUARDED_PORT_DESCRIPTOR => 16,
            _ => return,
        };
        let Some(descriptor) = descriptors.get_mut(at..at + length) else {
            return;
        };
        match kind {
            MACH_MSG_OOL_DESCRIPTOR | MACH_MSG_OOL_VOLATILE_DESCRIPTOR => {
                descriptor[8] = 1;
                descriptor[11] = MACH_MSG_OOL_DESCRIPTOR;
            }
            MACH_MSG_OOL_PORTS_DESCRIPTOR if descriptor[..8] == [0; 8] => descriptor[12..].fill(0),
            _ => {}
        }
        at += length;
    }
}

//! Turning a simulator (`docs/devices.md` 4): a `GSEvent` to the simulator's workspace port, laid
//! out as idb's `GSEvent.h` has it, with the orientation `UIDeviceOrientation` names. The surface
//! stays as it is: the simulator draws the turned screen turned inside it.

use koyo_device_wire::Orientation;

unsafe extern "C" {
    fn mach_msg_send(message: *mut u8) -> i32;
}

/// `UIDeviceOrientation`'s number for `orientation`.
pub fn number(orientation: Orientation) -> u32 {
    match orientation {
        Orientation::Portrait => 1,
        Orientation::PortraitUpsideDown => 2,
        Orientation::LandscapeLeft => 3,
        Orientation::LandscapeRight => 4,
    }
}

/// The message that turns the simulator, to the workspace port `port`.
fn message(port: u32, orientation: Orientation) -> [u8; 112] {
    const COPY_SEND: u32 = 19;
    let mut message = [0u8; 112];
    let mut put = |at: usize, value: u32| message[at..at + 4].copy_from_slice(&value.to_le_bytes());
    // The header: bits, size, the port it goes to, and the id of a GSEvent.
    put(0, COPY_SEND);
    put(4, 108);
    put(8, port);
    put(20, 0x7B);
    // The event's kind (an orientation change, from the system), its record's size, and what
    // it carries.
    put(0x18, 0x32 | 0x20000);
    put(0x48, 4);
    put(0x4C, number(orientation));
    message
}

/// Sends the turn; the kernel's answer, 0 when it went.
pub fn send(port: u32, orientation: Orientation) -> i32 {
    let mut message = message(port, orientation);
    // SAFETY: a whole message, its header first, of the size it says, to a send right we hold.
    unsafe { mach_msg_send(message.as_mut_ptr()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_is_laid_out_as_the_poc_sent_it() {
        let m = message(0x1234, Orientation::LandscapeRight);
        assert_eq!(&m[0..4], &19u32.to_le_bytes());
        assert_eq!(&m[4..8], &108u32.to_le_bytes());
        assert_eq!(&m[8..12], &0x1234u32.to_le_bytes());
        assert_eq!(&m[20..24], &0x7Bu32.to_le_bytes());
        assert_eq!(&m[0x18..0x1C], &(0x32u32 | 0x20000).to_le_bytes());
        assert_eq!(&m[0x48..0x4C], &4u32.to_le_bytes());
        assert_eq!(&m[0x4C..0x50], &4u32.to_le_bytes());
    }
}
